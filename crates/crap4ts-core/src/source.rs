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
        pending_kind: None,
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
    pending_kind: Option<(FunctionKind, String)>,
    units: Vec<FunctionUnit>,
}

impl<'path, 'source> FunctionCollector<'path, 'source> {
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
            let (kind, pending_name) = self.pending_kind.take().map_or_else(
                || {
                    let kind = if flags.is_constructor() {
                        FunctionKind::Constructor
                    } else if flags.contains(ScopeFlags::GetAccessor) {
                        FunctionKind::Getter
                    } else if flags.contains(ScopeFlags::SetAccessor) {
                        FunctionKind::Setter
                    } else if function.r#type == FunctionType::FunctionDeclaration {
                        FunctionKind::FunctionDeclaration
                    } else {
                        FunctionKind::FunctionExpression
                    };
                    (kind, None)
                },
                |(kind, name)| (kind, Some(name)),
            );
            let name = pending_name
                .as_deref()
                .or_else(|| function.id.as_ref().map(|id| id.name.as_str()));
            self.units
                .push(self.function_unit(function.span, body.span, name, kind, body));
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_arrow_function_expression(&mut self, function: &ArrowFunctionExpression<'ast>) {
        self.units.push(self.function_unit(
            function.span,
            function.body.span,
            None,
            FunctionKind::Arrow,
            &function.body,
        ));
        walk::walk_arrow_function_expression(self, function);
    }

    fn visit_method_definition(&mut self, method: &MethodDefinition<'ast>) {
        let kind = match method.kind {
            MethodDefinitionKind::Constructor => FunctionKind::Constructor,
            MethodDefinitionKind::Get => FunctionKind::Getter,
            MethodDefinitionKind::Set => FunctionKind::Setter,
            MethodDefinitionKind::Method => FunctionKind::Method,
        };
        let name = property_key_name(&method.key).map_or_else(
            || format!("<anonymous>@{}", method.span.start),
            str::to_string,
        );
        let previous = self.pending_kind.replace((kind, name));
        walk::walk_method_definition(self, method);
        self.pending_kind = previous;
    }

    fn visit_object_property(&mut self, property: &ObjectProperty<'ast>) {
        if property.method {
            let kind = match property.kind {
                PropertyKind::Get => FunctionKind::Getter,
                PropertyKind::Set => FunctionKind::Setter,
                PropertyKind::Init => FunctionKind::Method,
            };
            let name = property_key_name(&property.key).map_or_else(
                || format!("<anonymous>@{}", property.span.start),
                str::to_string,
            );
            let previous = self.pending_kind.replace((kind, name));
            walk::walk_object_property(self, property);
            self.pending_kind = previous;
        } else {
            walk::walk_object_property(self, property);
        }
    }
}

fn property_key_name<'a>(key: &'a PropertyKey<'_>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.as_str()),
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
