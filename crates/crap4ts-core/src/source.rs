//! Oxc-backed TypeScript source analysis.

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use oxc_syntax::{operator::AssignmentOperator, scope::ScopeFlags};

use crate::domain::{
    Complexity, CoreError, FunctionKind, FunctionUnit, ProjectRelativePath, SourcePosition,
    SourceRange,
};

/// Parse one TypeScript or TSX source file with Oxc and return normalized
/// executable units. Bodyless overloads and ambient declarations are omitted.
pub fn analyze_source(
    path: &ProjectRelativePath,
    source: &str,
) -> Result<Vec<FunctionUnit>, CoreError> {
    let source_type = match Path::new(path.as_str())
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("tsx") => SourceType::tsx(),
        Some("ts") | Some("mts") | Some("cts") => SourceType::ts(),
        _ => return Err(CoreError::UnsupportedSource(path.to_string())),
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() || parsed.panicked {
        return Err(CoreError::SourceParsing {
            path: path.to_string(),
            message: format!("{} syntax diagnostic(s)", parsed.diagnostics.len()),
        });
    }

    let starts = line_starts(source);
    let mut collector = FunctionCollector {
        path,
        starts: &starts,
        contexts: Vec::new(),
        units: Vec::new(),
    };
    collector.visit_program(&parsed.program);
    collector.units.sort_by(|left, right| {
        left.range
            .start
            .offset
            .cmp(&right.range.start.offset)
            .then_with(|| left.range.end.offset.cmp(&right.range.end.offset))
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(collector.units)
}

fn line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (offset, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(offset + 1);
        }
    }
    starts
}

fn source_position(starts: &[usize], offset: usize) -> SourcePosition {
    let line_index = starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1);
    SourcePosition {
        offset,
        line: line_index + 1,
        column: offset.saturating_sub(starts[line_index]),
    }
}

fn span_range(span: Span, starts: &[usize]) -> SourceRange {
    SourceRange {
        start: source_position(starts, span.start as usize),
        end: source_position(starts, span.end as usize),
    }
}

struct FunctionCollector<'path, 'source> {
    path: &'path ProjectRelativePath,
    starts: &'source [usize],
    contexts: Vec<FunctionContext>,
    units: Vec<FunctionUnit>,
}

/// Context inferred from the construct that owns a function-like expression.
///
/// Oxc stores function expressions and arrows independently from their
/// surrounding variable/property/assignment nodes. Keeping this context keyed
/// by the expression's exact span lets the collector infer useful names without
/// allowing a function in a computed key or nested expression to consume the
/// context intended for a different function.
#[derive(Clone, Debug)]
struct FunctionContext {
    span: Span,
    kind: Option<FunctionKind>,
    name: Option<String>,
}

impl FunctionContext {
    fn new(span: Span, kind: Option<FunctionKind>, name: Option<String>) -> Self {
        Self { span, kind, name }
    }
}

impl<'path, 'source> FunctionCollector<'path, 'source> {
    fn context_for(&self, span: Span) -> Option<FunctionContext> {
        self.contexts
            .iter()
            .rev()
            .find(|context| context.span == span)
            .cloned()
    }

    fn walk_with_context(
        &mut self,
        context: Option<FunctionContext>,
        walk: impl FnOnce(&mut Self),
    ) {
        let has_context = if let Some(context) = context {
            self.contexts.push(context);
            true
        } else {
            false
        };
        walk(self);
        if has_context {
            debug_assert!(self.contexts.pop().is_some());
        }
    }

    fn function_unit(
        &self,
        span: Span,
        body_span: Span,
        name: Option<&str>,
        kind: FunctionKind,
        body: &FunctionBody<'_>,
    ) -> FunctionUnit {
        let range = span_range(span, self.starts);
        let body_range = span_range(body_span, self.starts);
        let complexity = complexity_for_body(body);
        let display_name = name.filter(|name| !name.is_empty()).map_or_else(
            || format!("<anonymous>@{}:{}", range.start.line, range.start.column),
            str::to_string,
        );
        let id = format!(
            "{}::{}@{}-{}",
            self.path, display_name, range.start.offset, range.end.offset
        );
        FunctionUnit {
            id,
            path: self.path.clone(),
            name: display_name,
            kind,
            range,
            body_range,
            complexity,
        }
    }
}

impl<'ast, 'path, 'source> Visit<'ast> for FunctionCollector<'path, 'source> {
    fn visit_function(&mut self, function: &Function<'ast>, flags: ScopeFlags) {
        if let Some(body) = function.body.as_deref() {
            let context = self.context_for(function.span);
            let kind = context
                .as_ref()
                .and_then(|context| context.kind)
                .unwrap_or_else(|| function_kind(function, flags));
            // An explicit function identifier is the most precise name. The
            // surrounding binding/property name is used for anonymous
            // function expressions only.
            let name = function
                .id
                .as_ref()
                .map(|id| id.name.as_str())
                .or_else(|| context.as_ref().and_then(|context| context.name.as_deref()));
            self.units
                .push(self.function_unit(function.span, body.span, name, kind, body));
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_arrow_function_expression(&mut self, function: &ArrowFunctionExpression<'ast>) {
        let context = self.context_for(function.span);
        let name = context.as_ref().and_then(|context| context.name.as_deref());
        self.units.push(self.function_unit(
            function.span,
            function.body.span,
            name,
            FunctionKind::Arrow,
            &function.body,
        ));
        walk::walk_arrow_function_expression(self, function);
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'ast>) {
        let context = declarator
            .init
            .as_ref()
            .and_then(function_like)
            .map(|(span, _kind)| FunctionContext::new(span, None, binding_name(&declarator.id)));
        self.walk_with_context(context, |collector| {
            walk::walk_variable_declarator(collector, declarator);
        });
    }

    fn visit_formal_parameter(&mut self, parameter: &FormalParameter<'ast>) {
        let context = parameter
            .initializer
            .as_ref()
            .and_then(|expression| function_like(expression))
            .map(|(span, _kind)| {
                FunctionContext::new(span, None, binding_name(&parameter.pattern))
            });
        self.walk_with_context(context, |collector| {
            walk::walk_formal_parameter(collector, parameter);
        });
    }

    fn visit_assignment_pattern(&mut self, pattern: &AssignmentPattern<'ast>) {
        let context = function_like(&pattern.right)
            .map(|(span, _kind)| FunctionContext::new(span, None, binding_name(&pattern.left)));
        self.walk_with_context(context, |collector| {
            walk::walk_assignment_pattern(collector, pattern);
        });
    }

    fn visit_assignment_expression(&mut self, expression: &AssignmentExpression<'ast>) {
        let context = function_like(&expression.right).map(|(span, _kind)| {
            FunctionContext::new(
                span,
                None,
                expression.left.get_identifier_name().map(str::to_string),
            )
        });
        self.walk_with_context(context, |collector| {
            walk::walk_assignment_expression(collector, expression);
        });
    }

    fn visit_method_definition(&mut self, method: &MethodDefinition<'ast>) {
        let kind = match method.kind {
            MethodDefinitionKind::Constructor => FunctionKind::Constructor,
            MethodDefinitionKind::Get => FunctionKind::Getter,
            MethodDefinitionKind::Set => FunctionKind::Setter,
            MethodDefinitionKind::Method => FunctionKind::Method,
        };
        let context = FunctionContext::new(
            method.value.span,
            Some(kind),
            property_key_name(&method.key),
        );
        self.walk_with_context(Some(context), |collector| {
            walk::walk_method_definition(collector, method);
        });
    }

    fn visit_object_property(&mut self, property: &ObjectProperty<'ast>) {
        let context = function_like(&property.value).map(|(span, expression_kind)| {
            let kind = if property.method {
                Some(match property.kind {
                    PropertyKind::Get => FunctionKind::Getter,
                    PropertyKind::Set => FunctionKind::Setter,
                    PropertyKind::Init => FunctionKind::Method,
                })
            } else {
                match property.kind {
                    PropertyKind::Get => Some(FunctionKind::Getter),
                    PropertyKind::Set => Some(FunctionKind::Setter),
                    PropertyKind::Init => Some(expression_kind),
                }
            };
            FunctionContext::new(span, kind, property_key_name(&property.key))
        });
        self.walk_with_context(context, |collector| {
            walk::walk_object_property(collector, property);
        });
    }

    fn visit_property_definition(&mut self, property: &PropertyDefinition<'ast>) {
        let context = property
            .value
            .as_ref()
            .and_then(function_like)
            .map(|(span, kind)| {
                FunctionContext::new(span, Some(kind), property_key_name(&property.key))
            });
        self.walk_with_context(context, |collector| {
            walk::walk_property_definition(collector, property);
        });
    }

    fn visit_accessor_property(&mut self, property: &AccessorProperty<'ast>) {
        let context = property
            .value
            .as_ref()
            .and_then(function_like)
            .map(|(span, kind)| {
                FunctionContext::new(span, Some(kind), property_key_name(&property.key))
            });
        self.walk_with_context(context, |collector| {
            walk::walk_accessor_property(collector, property);
        });
    }

    fn visit_jsx_attribute(&mut self, attribute: &JSXAttribute<'ast>) {
        let context = attribute
            .value
            .as_ref()
            .and_then(|value| match value {
                JSXAttributeValue::ExpressionContainer(container) => {
                    container.expression.as_expression().and_then(function_like)
                }
                _ => None,
            })
            .map(|(span, kind)| {
                FunctionContext::new(
                    span,
                    Some(kind),
                    Some(attribute.name.get_identifier().name.as_str().to_string()),
                )
            });
        self.walk_with_context(context, |collector| {
            walk::walk_jsx_attribute(collector, attribute);
        });
    }
}

fn function_kind(function: &Function<'_>, flags: ScopeFlags) -> FunctionKind {
    if flags.is_constructor() {
        FunctionKind::Constructor
    } else if flags.contains(ScopeFlags::GetAccessor) {
        FunctionKind::Getter
    } else if flags.contains(ScopeFlags::SetAccessor) {
        FunctionKind::Setter
    } else if function.r#type == FunctionType::FunctionDeclaration {
        FunctionKind::FunctionDeclaration
    } else {
        FunctionKind::FunctionExpression
    }
}

fn binding_name(pattern: &BindingPattern<'_>) -> Option<String> {
    pattern
        .get_identifier_name()
        .map(|identifier| identifier.as_str().to_string())
}

/// Return the innermost function-like expression and its default kind. Type
/// assertions and parentheses do not change the function identity, so they are
/// peeled while inferring names from bindings and properties.
fn function_like(expression: &Expression<'_>) -> Option<(Span, FunctionKind)> {
    match expression {
        Expression::FunctionExpression(function) => {
            Some((function.span, FunctionKind::FunctionExpression))
        }
        Expression::ArrowFunctionExpression(function) => Some((function.span, FunctionKind::Arrow)),
        Expression::ParenthesizedExpression(expression) => function_like(&expression.expression),
        Expression::TSAsExpression(expression) => function_like(&expression.expression),
        Expression::TSSatisfiesExpression(expression) => function_like(&expression.expression),
        Expression::TSTypeAssertion(expression) => function_like(&expression.expression),
        Expression::TSNonNullExpression(expression) => function_like(&expression.expression),
        Expression::TSInstantiationExpression(expression) => function_like(&expression.expression),
        _ => None,
    }
}

fn property_key_name(key: &PropertyKey<'_>) -> Option<String> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.as_str().to_string()),
        PropertyKey::PrivateIdentifier(identifier) => {
            Some(format!("#{}", identifier.name.as_str()))
        }
        PropertyKey::Identifier(identifier) => Some(identifier.name.as_str().to_string()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.as_str().to_string()),
        PropertyKey::TemplateLiteral(literal) if literal.quasis.len() == 1 => literal
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked)
            .map(|value| value.as_str().to_string()),
        PropertyKey::NumericLiteral(literal) => literal
            .raw
            .map(|value| value.as_str().to_string())
            .or_else(|| Some(literal.value.to_string())),
        PropertyKey::BigIntLiteral(literal) => literal.raw.map(|value| value.as_str().to_string()),
        _ => None,
    }
}

fn complexity_for_body(body: &FunctionBody<'_>) -> Complexity {
    let mut visitor = ComplexityVisitor { value: 1 };
    visitor.visit_function_body(body);
    Complexity::new(visitor.value).expect("complexity starts at one")
}

struct ComplexityVisitor {
    value: u32,
}

impl<'ast> Visit<'ast> for ComplexityVisitor {
    fn visit_function(&mut self, _function: &Function<'ast>, _flags: ScopeFlags) {}

    fn visit_arrow_function_expression(&mut self, _function: &ArrowFunctionExpression<'ast>) {}

    fn visit_if_statement(&mut self, statement: &IfStatement<'ast>) {
        self.value += 1;
        walk::walk_if_statement(self, statement);
    }

    fn visit_do_while_statement(&mut self, statement: &DoWhileStatement<'ast>) {
        self.value += 1;
        walk::walk_do_while_statement(self, statement);
    }

    fn visit_while_statement(&mut self, statement: &WhileStatement<'ast>) {
        self.value += 1;
        walk::walk_while_statement(self, statement);
    }

    fn visit_for_statement(&mut self, statement: &ForStatement<'ast>) {
        self.value += 1;
        walk::walk_for_statement(self, statement);
    }

    fn visit_for_in_statement(&mut self, statement: &ForInStatement<'ast>) {
        self.value += 1;
        walk::walk_for_in_statement(self, statement);
    }

    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'ast>) {
        self.value += 1;
        walk::walk_for_of_statement(self, statement);
    }

    fn visit_switch_case(&mut self, case: &SwitchCase<'ast>) {
        if case.test.is_some() {
            self.value += 1;
        }
        walk::walk_switch_case(self, case);
    }

    fn visit_catch_clause(&mut self, clause: &CatchClause<'ast>) {
        self.value += 1;
        walk::walk_catch_clause(self, clause);
    }

    fn visit_conditional_expression(&mut self, expression: &ConditionalExpression<'ast>) {
        self.value += 1;
        walk::walk_conditional_expression(self, expression);
    }

    fn visit_logical_expression(&mut self, expression: &LogicalExpression<'ast>) {
        self.value += 1;
        walk::walk_logical_expression(self, expression);
    }

    fn visit_assignment_expression(&mut self, expression: &AssignmentExpression<'ast>) {
        if expression.operator != AssignmentOperator::Assign && expression.operator.is_logical() {
            self.value += 1;
        }
        walk::walk_assignment_expression(self, expression);
    }
}
