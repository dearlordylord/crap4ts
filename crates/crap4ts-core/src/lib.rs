//! Library-neutral domain values and adapters for the `crap4ts` CLI.
//!
//! Oxc AST nodes and Istanbul JSON records are deliberately kept inside the
//! adapter implementation below. Reports and scores only expose the stable
//! values defined in this module.

use std::{cmp::Ordering, collections::BTreeMap, fmt, path::Path};

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use oxc_syntax::{operator::AssignmentOperator, scope::ScopeFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Version of the canonical JSON report document.
pub const REPORT_VERSION: u32 = 1;

/// A byte offset and human-readable source position.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourcePosition {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

/// An inclusive-start, exclusive-end source range.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl SourceRange {
    fn contains(&self, position: SourcePosition) -> bool {
        self.start.offset <= position.offset && position.offset < self.end.offset
    }

    fn size(self) -> usize {
        self.end.offset.saturating_sub(self.start.offset)
    }
}

/// Concrete executable function-like units recognized by the source adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionKind {
    Arrow,
    Constructor,
    FunctionDeclaration,
    FunctionExpression,
    Getter,
    Method,
    Setter,
}

impl FunctionKind {
    fn as_label(self) -> &'static str {
        match self {
            Self::Arrow => "arrow",
            Self::Constructor => "constructor",
            Self::FunctionDeclaration => "function declaration",
            Self::FunctionExpression => "function expression",
            Self::Getter => "getter",
            Self::Method => "method",
            Self::Setter => "setter",
        }
    }
}

/// Validated cyclomatic complexity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Complexity(u32);

impl Complexity {
    pub fn new(value: u32) -> Result<Self, CoreError> {
        if value == 0 {
            return Err(CoreError::InvalidComplexity(value));
        }
        Ok(Self(value))
    }

    pub const fn one() -> Self {
        Self(1)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Measured coverage, or explicit unavailable evidence.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Coverage {
    Measured {
        covered: u64,
        total: u64,
        fraction: f64,
    },
    Unknown {
        reason: String,
    },
}

impl Coverage {
    pub fn measured(covered: u64, total: u64) -> Result<Self, CoreError> {
        if total == 0 || covered > total {
            return Err(CoreError::InvalidCoverage { covered, total });
        }
        Ok(Self::Measured {
            covered,
            total,
            fraction: covered as f64 / total as f64,
        })
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Self::Unknown {
            reason: reason.into(),
        }
    }

    pub fn fraction(&self) -> Option<f64> {
        match self {
            Self::Measured { fraction, .. } => Some(*fraction),
            Self::Unknown { .. } => None,
        }
    }
}

/// A normalized source function. The parser-specific AST never crosses this
/// boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FunctionUnit {
    pub id: String,
    pub path: String,
    pub name: String,
    pub kind: FunctionKind,
    pub range: SourceRange,
    pub complexity: Complexity,
}

/// A scored or unavailable report row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReportRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub kind: FunctionKind,
    pub range: SourceRange,
    pub complexity: Complexity,
    pub coverage: Coverage,
    pub crap: Option<f64>,
}

/// Structured report diagnostic categories.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    Configuration,
    CoverageAttribution,
    CoverageParsing,
    MissingEvidence,
    SourceParsing,
    ThresholdBreach,
}

/// Deterministic user-facing diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub category: DiagnosticCategory,
    pub message: String,
}

impl Diagnostic {
    fn new(category: DiagnosticCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

/// Canonical report document. There is intentionally no timestamp.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub version: u32,
    pub threshold: u32,
    pub rows: Vec<ReportRow>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub fn gate_breached(&self) -> bool {
        self.rows
            .iter()
            .any(|row| row.crap.is_some_and(|score| score > self.threshold as f64))
    }
}

/// Project-relative source passed from the CLI into the source adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("unsupported source extension for `{0}` (expected .ts or .tsx)")]
    UnsupportedSource(String),
    #[error("source parsing failed for `{path}`: {message}")]
    SourceParsing { path: String, message: String },
    #[error("coverage parsing failed: {0}")]
    CoverageParsing(String),
    #[error("coverage attribution failed: {0}")]
    CoverageAttribution(String),
    #[error("missing coverage evidence for `{path}`: {reason}")]
    MissingEvidence { path: String, reason: String },
    #[error("complexity must be positive, got {0}")]
    InvalidComplexity(u32),
    #[error("invalid coverage counts: covered={covered}, total={total}")]
    InvalidCoverage { covered: u64, total: u64 },
    #[error("invalid coverage fraction: {0}")]
    InvalidCoverageFraction(f64),
    #[error("coverage artifact contains ambiguous file identity `{0}`")]
    AmbiguousCoverageFile(String),
}

/// Calculate `CRAP = CC² × (1 - coverage)³ + CC` for measured coverage.
pub fn crap_score(complexity: Complexity, coverage: f64) -> Result<f64, CoreError> {
    if !coverage.is_finite() || !(0.0..=1.0).contains(&coverage) {
        return Err(CoreError::InvalidCoverageFraction(coverage));
    }
    let cc = f64::from(complexity.get());
    Ok(cc * cc * (1.0 - coverage).powi(3) + cc)
}

/// Analyze source files and an existing Istanbul JSON artifact.
pub fn analyze(
    sources: &[SourceFile],
    coverage_json: &str,
    threshold: u32,
    allow_unknown: bool,
) -> Result<Report, CoreError> {
    let coverage = IstanbulCoverage::parse(coverage_json)?;
    let mut units = Vec::new();
    for source in sources {
        units.extend(analyze_source(&source.path, &source.source)?);
    }

    let mut rows = Vec::with_capacity(units.len());
    for unit in &units {
        let file = coverage.file_for(&unit.path)?;
        let measured = file.and_then(|file| {
            file.coverage_for(unit, sources_for_path(sources, &unit.path), &units)
        });
        let coverage = measured.unwrap_or_else(|| {
            Coverage::unknown("no matching Istanbul function or statement evidence")
        });
        if let Coverage::Unknown { reason } = &coverage {
            if !allow_unknown {
                return Err(CoreError::MissingEvidence {
                    path: unit.path.clone(),
                    reason: reason.clone(),
                });
            }
        }
        let crap = coverage.fraction().map(|fraction| {
            // The adapter validates fractions before they reach this point.
            crap_score(unit.complexity, fraction).expect("validated coverage fraction")
        });
        rows.push(ReportRow {
            id: unit.id.clone(),
            path: unit.path.clone(),
            name: unit.name.clone(),
            kind: unit.kind,
            range: unit.range,
            complexity: unit.complexity,
            coverage,
            crap,
        });
    }

    rows.sort_by(compare_rows);
    let mut diagnostics = Vec::new();
    if rows
        .iter()
        .any(|row| row.crap.is_some_and(|score| score > threshold as f64))
    {
        diagnostics.push(Diagnostic::new(
            DiagnosticCategory::ThresholdBreach,
            format!("one or more CRAP scores exceed the threshold of {threshold}"),
        ));
    }
    Ok(Report {
        version: REPORT_VERSION,
        threshold,
        rows,
        diagnostics,
    })
}

fn sources_for_path<'a>(sources: &'a [SourceFile], path: &str) -> &'a str {
    sources
        .iter()
        .find(|source| source.path == path)
        .map_or("", |source| &source.source)
}

fn compare_rows(left: &ReportRow, right: &ReportRow) -> Ordering {
    match (left.crap, right.crap) {
        (Some(left_score), Some(right_score)) => right_score
            .partial_cmp(&left_score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.range.start.offset.cmp(&right.range.start.offset))
            .then_with(|| left.range.end.offset.cmp(&right.range.end.offset))
            .then_with(|| left.name.cmp(&right.name)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left
            .path
            .cmp(&right.path)
            .then_with(|| left.range.start.offset.cmp(&right.range.start.offset))
            .then_with(|| left.range.end.offset.cmp(&right.range.end.offset))
            .then_with(|| left.name.cmp(&right.name)),
    }
}

/// Parse one TypeScript or TSX source file with Oxc and return normalized
/// executable units. Bodyless overloads and ambient declarations are omitted.
pub fn analyze_source(path: &str, source: &str) -> Result<Vec<FunctionUnit>, CoreError> {
    let source_type = match Path::new(path)
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
        source,
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

fn source_position(_source: &str, starts: &[usize], offset: usize) -> SourcePosition {
    let line_index = starts
        .partition_point(|start| *start <= offset)
        .saturating_sub(1);
    SourcePosition {
        offset,
        line: line_index + 1,
        column: offset.saturating_sub(starts[line_index]),
    }
}

fn span_range(span: Span, source: &str, starts: &[usize]) -> SourceRange {
    SourceRange {
        start: source_position(source, starts, span.start as usize),
        end: source_position(source, starts, span.end as usize),
    }
}

struct FunctionCollector<'path, 'source> {
    path: &'path str,
    source: &'source str,
    starts: &'source [usize],
    pending_kind: Option<(FunctionKind, String)>,
    units: Vec<FunctionUnit>,
}

impl<'path, 'source> FunctionCollector<'path, 'source> {
    fn function_unit(
        &self,
        span: Span,
        name: Option<&str>,
        kind: FunctionKind,
        body: &FunctionBody<'_>,
    ) -> FunctionUnit {
        let range = span_range(span, self.source, self.starts);
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
            path: self.path.to_string(),
            name: display_name,
            kind,
            range,
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
                .push(self.function_unit(function.span, name, kind, body));
        }
        walk::walk_function(self, function, flags);
    }

    fn visit_arrow_function_expression(&mut self, function: &ArrowFunctionExpression<'ast>) {
        self.units.push(self.function_unit(
            function.span,
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
    Complexity(visitor.value)
}

#[derive(Default)]
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

#[derive(Debug)]
struct IstanbulCoverage {
    files: BTreeMap<String, IstanbulFile>,
}

#[derive(Debug)]
struct IstanbulFile {
    function_entries: Vec<IstanbulFunction>,
    statement_entries: Vec<IstanbulStatement>,
}

#[derive(Debug)]
struct IstanbulFunction {
    name: String,
    range: IstanbulRange,
    hits: Option<u64>,
}

#[derive(Debug)]
struct IstanbulStatement {
    range: IstanbulRange,
    hits: u64,
}

#[derive(Clone, Copy, Debug)]
struct IstanbulPosition {
    line: usize,
    column: usize,
}

#[derive(Clone, Copy, Debug)]
struct IstanbulRange {
    start: IstanbulPosition,
    end: IstanbulPosition,
}

impl IstanbulCoverage {
    fn parse(input: &str) -> Result<Self, CoreError> {
        let value: Value = serde_json::from_str(input)
            .map_err(|error| CoreError::CoverageParsing(format!("invalid JSON: {error}")))?;
        let files = value.as_object().ok_or_else(|| {
            CoreError::CoverageParsing("top-level value must be an object".to_string())
        })?;
        let mut normalized = BTreeMap::new();
        for (key, value) in files {
            let object = value.as_object().ok_or_else(|| {
                CoreError::CoverageParsing(format!("coverage entry `{key}` must be an object"))
            })?;
            let identity = object.get("path").and_then(Value::as_str).unwrap_or(key);
            let identity = normalize_path(identity);
            if normalized.contains_key(&identity) {
                return Err(CoreError::AmbiguousCoverageFile(identity));
            }
            normalized.insert(
                identity,
                IstanbulFile {
                    function_entries: parse_functions(object)?,
                    statement_entries: parse_statements(object)?,
                },
            );
        }
        Ok(Self { files: normalized })
    }

    fn file_for(&self, path: &str) -> Result<Option<&IstanbulFile>, CoreError> {
        let identity = normalize_path(path);
        if let Some(file) = self.files.get(&identity) {
            return Ok(Some(file));
        }
        // Istanbul often writes absolute paths while source selection is
        // project-relative. A unique suffix match is equivalent to stripping
        // the project root; multiple matches fail closed.
        let suffix = format!("/{identity}");
        let matches = self
            .files
            .iter()
            .filter(|(key, _)| key.ends_with(&suffix))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => Ok(None),
            [(_, file)] => Ok(Some(file)),
            _ => Err(CoreError::AmbiguousCoverageFile(identity)),
        }
    }
}

impl IstanbulFile {
    fn coverage_for(
        &self,
        unit: &FunctionUnit,
        source: &str,
        units: &[FunctionUnit],
    ) -> Option<Coverage> {
        let function = self.function_entries.iter().find(|entry| {
            range_matches_unit(entry.range, unit.range, source)
                || (entry.name == unit.name && range_overlaps_unit(entry.range, unit.range, source))
        });
        let mut owned = Vec::new();
        for statement in &self.statement_entries {
            let Some(position) = source_position_from_istanbul(statement.range.start, source)
            else {
                continue;
            };
            let Some(owner) = units
                .iter()
                .filter(|candidate| {
                    candidate.path == unit.path && candidate.range.contains(position)
                })
                .min_by(|left, right| {
                    left.range
                        .size()
                        .cmp(&right.range.size())
                        .then_with(|| left.id.cmp(&right.id))
                })
            else {
                continue;
            };
            if owner.id == unit.id {
                owned.push(statement.hits);
            }
        }
        if !owned.is_empty() {
            let total = owned.len() as u64;
            let covered = owned.iter().filter(|hits| **hits > 0).count() as u64;
            return Coverage::measured(covered, total).ok();
        }
        function
            .and_then(|function| function.hits)
            .and_then(|hits| Coverage::measured(u64::from(hits > 0), 1).ok())
    }
}

fn range_matches_unit(range: IstanbulRange, unit: SourceRange, source: &str) -> bool {
    let (Some(start), Some(end)) = (
        source_position_from_istanbul(range.start, source),
        source_position_from_istanbul(range.end, source),
    ) else {
        return false;
    };
    (start.offset == unit.start.offset && end.offset == unit.end.offset)
        || (start.offset <= unit.start.offset && unit.end.offset <= end.offset)
        || (unit.start.offset <= start.offset && end.offset <= unit.end.offset)
}

fn range_overlaps_unit(range: IstanbulRange, unit: SourceRange, source: &str) -> bool {
    let (Some(start), Some(end)) = (
        source_position_from_istanbul(range.start, source),
        source_position_from_istanbul(range.end, source),
    ) else {
        return false;
    };
    start.offset < unit.end.offset && unit.start.offset < end.offset
}

fn source_position_from_istanbul(
    position: IstanbulPosition,
    source: &str,
) -> Option<SourcePosition> {
    if position.line == 0 {
        return None;
    }
    let starts = line_starts(source);
    let line_start = *starts.get(position.line - 1)?;
    let offset = line_start.checked_add(position.column)?;
    (offset <= source.len()).then(|| source_position(source, &starts, offset))
}

fn parse_functions(
    object: &serde_json::Map<String, Value>,
) -> Result<Vec<IstanbulFunction>, CoreError> {
    let Some(map) = object.get("fnMap") else {
        return Ok(Vec::new());
    };
    let map = map
        .as_object()
        .ok_or_else(|| CoreError::CoverageParsing("fnMap must be an object".to_string()))?;
    let counts = object.get("f").and_then(Value::as_object);
    let mut functions = Vec::new();
    for (id, value) in map {
        let entry = value.as_object().ok_or_else(|| {
            CoreError::CoverageParsing(format!("fnMap entry `{id}` must be an object"))
        })?;
        let range = entry
            .get("loc")
            .or_else(|| entry.get("decl"))
            .and_then(parse_range)
            .ok_or_else(|| {
                CoreError::CoverageParsing(format!("fnMap entry `{id}` has no valid loc"))
            })?;
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let hits = counts
            .and_then(|counts| counts.get(id))
            .and_then(Value::as_u64);
        functions.push(IstanbulFunction { name, range, hits });
    }
    Ok(functions)
}

fn parse_statements(
    object: &serde_json::Map<String, Value>,
) -> Result<Vec<IstanbulStatement>, CoreError> {
    let Some(map) = object.get("statementMap") else {
        return Ok(Vec::new());
    };
    let map = map
        .as_object()
        .ok_or_else(|| CoreError::CoverageParsing("statementMap must be an object".to_string()))?;
    let counts = object.get("s").and_then(Value::as_object).ok_or_else(|| {
        CoreError::CoverageParsing("statementMap requires an `s` count object".to_string())
    })?;
    let mut statements = Vec::new();
    for (id, value) in map {
        let range = value
            .as_object()
            .and_then(|object| parse_range(&Value::Object(object.clone())))
            .ok_or_else(|| {
                CoreError::CoverageParsing(format!("statementMap entry `{id}` has no valid range"))
            })?;
        let hits = counts.get(id).and_then(Value::as_u64).ok_or_else(|| {
            CoreError::CoverageParsing(format!(
                "statement count `{id}` must be an unsigned integer"
            ))
        })?;
        statements.push(IstanbulStatement { range, hits });
    }
    Ok(statements)
}

fn parse_range(value: &Value) -> Option<IstanbulRange> {
    let object = value.as_object()?;
    Some(IstanbulRange {
        start: parse_position(object.get("start")?)?,
        end: parse_position(object.get("end")?)?,
    })
}

fn parse_position(value: &Value) -> Option<IstanbulPosition> {
    let object = value.as_object()?;
    Some(IstanbulPosition {
        line: usize::try_from(object.get("line")?.as_u64()?).ok()?,
        column: usize::try_from(object.get("column")?.as_u64()?).ok()?,
    })
}

fn normalize_path(path: &str) -> String {
    let mut result = String::new();
    for component in path.replace('\\', "/").split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(component);
    }
    result
}

/// Render a report in the stable human-readable format.
pub fn render_text(report: &Report) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "crap4ts report v{} (threshold: {})\n",
        report.version, report.threshold
    ));
    for row in &report.rows {
        let range = format!(
            "{}:{}-{}:{}",
            row.range.start.line, row.range.start.column, row.range.end.line, row.range.end.column
        );
        let coverage = match &row.coverage {
            Coverage::Measured { fraction, .. } => format!("{:.2}%", fraction * 100.0),
            Coverage::Unknown { reason } => format!("unknown ({reason})"),
        };
        let score = row
            .crap
            .map_or_else(|| "unknown".to_string(), |score| format!("{score:.6}"));
        output.push_str(&format!(
            "{} {} [{}] {} complexity={} coverage={} crap={}\n",
            row.path,
            range,
            row.kind.as_label(),
            row.name,
            row.complexity.get(),
            coverage,
            score
        ));
    }
    if report.rows.is_empty() {
        output.push_str("(no executable TypeScript functions)\n");
    }
    output
}

impl fmt::Display for DiagnosticCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Configuration => "configuration",
            Self::CoverageAttribution => "coverage attribution",
            Self::CoverageParsing => "coverage parsing",
            Self::MissingEvidence => "missing evidence",
            Self::SourceParsing => "source parsing",
            Self::ThresholdBreach => "threshold breach",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_crap_vectors() {
        assert_eq!(crap_score(Complexity::one(), 1.0).unwrap(), 1.0);
        assert_eq!(crap_score(Complexity::one(), 0.0).unwrap(), 2.0);
        assert_eq!(crap_score(Complexity::new(2).unwrap(), 0.5).unwrap(), 2.5);
    }

    #[test]
    fn complexity_does_not_include_nested_function_body() {
        let source = "function outer() { if (true) { return () => { if (false) return 1; }; } }";
        let units = analyze_source("fixture.ts", source).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].name, "outer");
        assert_eq!(units[0].complexity.get(), 2);
        assert_eq!(units[1].kind, FunctionKind::Arrow);
        assert_eq!(units[1].complexity.get(), 2);
    }

    #[test]
    fn methods_and_accessors_are_distinct_units() {
        let source = "class Counter { value() { if (true) return 1; } get current() { return 1; } set current(value) { this.value = value; } }";
        let units = analyze_source("fixture.ts", source).unwrap();
        assert_eq!(units.len(), 3);
        assert_eq!(units[0].kind, FunctionKind::Method);
        assert_eq!(units[0].name, "value");
        assert_eq!(units[1].kind, FunctionKind::Getter);
        assert_eq!(units[1].name, "current");
        assert_eq!(units[2].kind, FunctionKind::Setter);
        assert_eq!(units[2].name, "current");
    }

    #[test]
    fn report_orders_numeric_rows_before_unknown_rows() {
        let measured = Coverage::measured(1, 1).unwrap();
        let row = |path: &str, crap: Option<f64>| ReportRow {
            id: path.to_string(),
            path: path.to_string(),
            name: "f".to_string(),
            kind: FunctionKind::FunctionDeclaration,
            range: SourceRange {
                start: SourcePosition {
                    offset: 0,
                    line: 1,
                    column: 0,
                },
                end: SourcePosition {
                    offset: 1,
                    line: 1,
                    column: 1,
                },
            },
            complexity: Complexity::one(),
            coverage: if crap.is_some() {
                measured.clone()
            } else {
                Coverage::unknown("missing")
            },
            crap,
        };
        let mut rows = [
            row("z.ts", None),
            row("a.ts", Some(2.0)),
            row("b.ts", Some(3.0)),
        ];
        rows.sort_by(compare_rows);
        assert_eq!(
            rows.iter().map(|row| row.path.as_str()).collect::<Vec<_>>(),
            ["b.ts", "a.ts", "z.ts"]
        );
    }
}
