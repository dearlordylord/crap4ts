//! Istanbul JSON adapter and exclusive source-range attribution.

use std::{collections::BTreeMap, path::Path};

use serde_json::{Map, Value};

use crate::domain::{
    CoreError, Coverage, FunctionUnit, ProjectRelativePath, SourceFile, SourcePosition, SourceRange,
};

#[derive(Debug)]
pub(crate) struct IstanbulCoverage {
    files: BTreeMap<ProjectRelativePath, IstanbulFile>,
}

/// Normalized coverage boundary consumed by scoring. A second format such as
/// LCOV can implement this contract without changing the application or
/// renderers.
pub(crate) trait CoverageAdapter {
    fn validate_for_source(
        &self,
        path: &ProjectRelativePath,
        source: &str,
    ) -> Result<(), CoreError>;

    fn validate_attribution(
        &self,
        units: &[FunctionUnit],
        sources: &[SourceFile],
    ) -> Result<(), CoreError>;

    fn coverage_for(
        &self,
        path: &ProjectRelativePath,
        unit: &FunctionUnit,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<Option<Coverage>, CoreError>;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IstanbulPosition {
    line: usize,
    column: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IstanbulRange {
    start: IstanbulPosition,
    end: IstanbulPosition,
}

impl IstanbulCoverage {
    pub(crate) fn parse(input: &str, root: &Path) -> Result<Self, CoreError> {
        let value: Value = serde_json::from_str(input)
            .map_err(|error| CoreError::CoverageParsing(format!("invalid JSON: {error}")))?;
        let files = value.as_object().ok_or_else(|| {
            CoreError::CoverageParsing("top-level value must be an object".to_string())
        })?;
        let mut normalized = BTreeMap::new();
        for (key, value) in files {
            let object = value.as_object().ok_or_else(|| {
                CoreError::CoverageParsing(format!("coverage entry '{key}' must be an object"))
            })?;
            let raw_identity = match object.get("path") {
                Some(value) => value.as_str().ok_or_else(|| {
                    CoreError::CoverageParsing(format!(
                        "coverage entry '{key}' path must be a string"
                    ))
                })?,
                None => key,
            };
            let identity = ProjectRelativePath::from_artifact_path(raw_identity, root)
                .map_err(|error| CoreError::CoverageParsing(error.to_string()))?;
            if normalized.contains_key(&identity) {
                return Err(CoreError::AmbiguousCoverageFile(identity.to_string()));
            }
            validate_branch_counts(object)?;
            let file = IstanbulFile {
                function_entries: parse_functions(object)?,
                statement_entries: parse_statements(object)?,
            };
            normalized.insert(identity, file);
        }
        Ok(Self { files: normalized })
    }

    pub(crate) fn validate_for_source(
        &self,
        path: &ProjectRelativePath,
        source: &str,
    ) -> Result<(), CoreError> {
        if let Some(file) = self.files.get(path) {
            file.validate_positions(source)?;
        }
        Ok(())
    }

    pub(crate) fn coverage_for(
        &self,
        path: &ProjectRelativePath,
        unit: &FunctionUnit,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<Option<Coverage>, CoreError> {
        let coverage = self
            .files
            .get(path)
            .map(|file| file.coverage_for(unit, source, units))
            .transpose()?
            .flatten();
        Ok(coverage)
    }

    pub(crate) fn validate_attribution(
        &self,
        units: &[FunctionUnit],
        sources: &[SourceFile],
    ) -> Result<(), CoreError> {
        let mut owners = BTreeMap::new();
        for unit in units {
            let Some(file) = self.files.get(&unit.path) else {
                continue;
            };
            let source = sources
                .iter()
                .find(|source| source.path == unit.path)
                .map_or("", |source| source.source.as_str());
            let Some(index) = file.matching_function(unit, source)? else {
                continue;
            };
            if let Some(previous) = owners.insert((unit.path.clone(), index), unit.id.clone()) {
                return Err(CoreError::CoverageAttribution(format!(
                    "Istanbul function identity {index} in '{}' matches both '{}' and '{}'",
                    unit.path, previous, unit.id
                )));
            }
        }
        Ok(())
    }
}

impl CoverageAdapter for IstanbulCoverage {
    fn validate_for_source(
        &self,
        path: &ProjectRelativePath,
        source: &str,
    ) -> Result<(), CoreError> {
        IstanbulCoverage::validate_for_source(self, path, source)
    }

    fn validate_attribution(
        &self,
        units: &[FunctionUnit],
        sources: &[SourceFile],
    ) -> Result<(), CoreError> {
        IstanbulCoverage::validate_attribution(self, units, sources)
    }

    fn coverage_for(
        &self,
        path: &ProjectRelativePath,
        unit: &FunctionUnit,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<Option<Coverage>, CoreError> {
        IstanbulCoverage::coverage_for(self, path, unit, source, units)
    }
}

impl IstanbulFile {
    fn validate_positions(&self, source: &str) -> Result<(), CoreError> {
        for function in &self.function_entries {
            source_range_from_istanbul(function.range, source)?;
        }
        for statement in &self.statement_entries {
            source_range_from_istanbul(statement.range, source)?;
        }
        Ok(())
    }

    fn matching_function(
        &self,
        unit: &FunctionUnit,
        source: &str,
    ) -> Result<Option<usize>, CoreError> {
        let mut exact = Vec::new();
        for (index, function) in self.function_entries.iter().enumerate() {
            let range = source_range_from_istanbul(function.range, source)?;
            if range == unit.body_range || range == unit.range {
                exact.push(index);
            }
        }
        if exact.len() > 1 {
            return Err(CoreError::CoverageAttribution(format!(
                "multiple Istanbul functions exactly match '{}'",
                unit.id
            )));
        }
        if let Some(index) = exact.first() {
            return Ok(Some(*index));
        }

        let mut named = Vec::new();
        for (index, function) in self.function_entries.iter().enumerate() {
            if function.name != unit.name {
                continue;
            }
            let range = source_range_from_istanbul(function.range, source)?;
            if ranges_overlap(range, unit.range) {
                named.push(index);
            }
        }
        if named.len() > 1 {
            return Err(CoreError::CoverageAttribution(format!(
                "ambiguous Istanbul function match for '{}'",
                unit.id
            )));
        }
        Ok(named.first().copied())
    }

    pub(crate) fn coverage_for(
        &self,
        unit: &FunctionUnit,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<Option<Coverage>, CoreError> {
        let function = self.matching_function(unit, source)?;
        let mut owned = Vec::new();
        for statement in &self.statement_entries {
            let position = source_position_from_istanbul(statement.range.start, source)?;
            let candidates = units
                .iter()
                .filter(|candidate| {
                    candidate.path == unit.path && candidate.range.contains(position)
                })
                .collect::<Vec<_>>();
            let Some(owner) = candidates.iter().min_by(|left, right| {
                left.range
                    .size()
                    .cmp(&right.range.size())
                    .then_with(|| left.id.cmp(&right.id))
            }) else {
                continue;
            };
            if candidates.iter().any(|candidate| {
                candidate.id != owner.id && candidate.range.size() == owner.range.size()
            }) {
                return Err(CoreError::CoverageAttribution(format!(
                    "ambiguous statement ownership in '{}'",
                    unit.path
                )));
            }
            if owner.id == unit.id {
                owned.push(statement.hits);
            }
        }
        if !owned.is_empty() {
            let total = owned.len() as u64;
            let covered = owned.iter().filter(|hits| **hits > 0).count() as u64;
            return Coverage::measured(covered, total).map(Some);
        }
        Ok(function
            .and_then(|index| self.function_entries[index].hits)
            .and_then(|hits| Coverage::measured(u64::from(hits > 0), 1).ok()))
    }
}

fn parse_functions(object: &Map<String, Value>) -> Result<Vec<IstanbulFunction>, CoreError> {
    let counts = object
        .get("f")
        .map(|value| parse_count_object(value, "f"))
        .transpose()?;
    let Some(map_value) = object.get("fnMap") else {
        return Ok(Vec::new());
    };
    let map = map_value
        .as_object()
        .ok_or_else(|| CoreError::CoverageParsing("fnMap must be an object".to_string()))?;
    let mut functions = Vec::new();
    for (id, value) in map {
        let entry = value.as_object().ok_or_else(|| {
            CoreError::CoverageParsing(format!("fnMap entry '{id}' must be an object"))
        })?;
        let range_value = entry
            .get("loc")
            .or_else(|| entry.get("decl"))
            .ok_or_else(|| CoreError::CoverageParsing(format!("fnMap entry '{id}' has no loc")))?;
        let range = parse_range(range_value, &format!("fnMap entry '{id}'"))?;
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let hits = counts
            .as_ref()
            .map(|counts| {
                counts.get(id).copied().ok_or_else(|| {
                    CoreError::CoverageParsing(format!("f is missing count for function '{id}'"))
                })
            })
            .transpose()?;
        functions.push(IstanbulFunction { name, range, hits });
    }
    Ok(functions)
}

fn parse_statements(object: &Map<String, Value>) -> Result<Vec<IstanbulStatement>, CoreError> {
    let counts = object
        .get("s")
        .map(|value| parse_count_object(value, "s"))
        .transpose()?;
    let Some(map_value) = object.get("statementMap") else {
        return Ok(Vec::new());
    };
    let map = map_value
        .as_object()
        .ok_or_else(|| CoreError::CoverageParsing("statementMap must be an object".to_string()))?;
    let counts = counts.ok_or_else(|| {
        CoreError::CoverageParsing("statementMap requires an 's' count object".to_string())
    })?;
    let mut statements = Vec::new();
    for (id, value) in map {
        let range = parse_range(value, &format!("statementMap entry '{id}'"))?;
        let hits = counts.get(id).copied().ok_or_else(|| {
            CoreError::CoverageParsing(format!("s is missing count for statement '{id}'"))
        })?;
        statements.push(IstanbulStatement { range, hits });
    }
    if counts.keys().any(|id| !map.contains_key(id)) {
        return Err(CoreError::CoverageParsing(
            "s contains a statement count without a statementMap entry".to_string(),
        ));
    }
    Ok(statements)
}

fn parse_count_object(value: &Value, label: &str) -> Result<BTreeMap<String, u64>, CoreError> {
    let map = value.as_object().ok_or_else(|| {
        CoreError::CoverageParsing(format!("{label} must be an object of nonnegative integers"))
    })?;
    let mut counts = BTreeMap::new();
    for (id, value) in map {
        let count = value.as_u64().ok_or_else(|| {
            CoreError::CoverageParsing(format!(
                "{label} count '{id}' must be a nonnegative integer"
            ))
        })?;
        counts.insert(id.clone(), count);
    }
    Ok(counts)
}

fn validate_branch_counts(object: &Map<String, Value>) -> Result<(), CoreError> {
    let Some(value) = object.get("b") else {
        return Ok(());
    };
    let map = value.as_object().ok_or_else(|| {
        CoreError::CoverageParsing("b must be an object of nonnegative integer arrays".to_string())
    })?;
    for (id, value) in map {
        let values = value.as_array().ok_or_else(|| {
            CoreError::CoverageParsing(format!("b count '{id}' must be an array"))
        })?;
        for (index, count) in values.iter().enumerate() {
            if count.as_u64().is_none() {
                return Err(CoreError::CoverageParsing(format!(
                    "b count '{id}[{index}]' must be a nonnegative integer"
                )));
            }
        }
    }
    Ok(())
}

fn parse_range(value: &Value, context: &str) -> Result<IstanbulRange, CoreError> {
    let object = value
        .as_object()
        .ok_or_else(|| CoreError::CoverageParsing(format!("{context} range must be an object")))?;
    let start = parse_position(
        object
            .get("start")
            .ok_or_else(|| CoreError::CoverageParsing(format!("{context} has no start")))?,
        context,
    )?;
    let end = parse_position(
        object
            .get("end")
            .ok_or_else(|| CoreError::CoverageParsing(format!("{context} has no end")))?,
        context,
    )?;
    if start.line > end.line || (start.line == end.line && start.column >= end.column) {
        return Err(CoreError::CoverageParsing(format!(
            "{context} range ends before it starts"
        )));
    }
    Ok(IstanbulRange { start, end })
}

fn parse_position(value: &Value, context: &str) -> Result<IstanbulPosition, CoreError> {
    let object = value.as_object().ok_or_else(|| {
        CoreError::CoverageParsing(format!("{context} position must be an object"))
    })?;
    let line = object
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| {
            CoreError::CoverageParsing(format!("{context} line must be a positive integer"))
        })?;
    let column = object
        .get("column")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| {
            CoreError::CoverageParsing(format!("{context} column must be a nonnegative integer"))
        })?;
    if line == 0 {
        return Err(CoreError::CoverageParsing(format!(
            "{context} line must be at least 1"
        )));
    }
    Ok(IstanbulPosition { line, column })
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

fn source_position_from_istanbul(
    position: IstanbulPosition,
    source: &str,
) -> Result<SourcePosition, CoreError> {
    let starts = line_starts(source);
    let line_start = *starts.get(position.line - 1).ok_or_else(|| {
        CoreError::CoverageParsing(format!(
            "coverage line {} is outside the source",
            position.line
        ))
    })?;
    let line_end = starts.get(position.line).copied().unwrap_or(source.len());
    let mut content_end = line_end;
    while content_end > line_start && matches!(source.as_bytes()[content_end - 1], b'\n' | b'\r') {
        content_end -= 1;
    }
    let line_length = content_end - line_start;
    if position.column > line_length {
        return Err(CoreError::CoverageParsing(format!(
            "coverage column {} is outside source line {}",
            position.column, position.line
        )));
    }
    let offset = line_start + position.column;
    Ok(SourcePosition {
        offset,
        line: position.line,
        column: position.column,
    })
}

fn source_range_from_istanbul(
    range: IstanbulRange,
    source: &str,
) -> Result<SourceRange, CoreError> {
    let start = source_position_from_istanbul(range.start, source)?;
    let end = source_position_from_istanbul(range.end, source)?;
    if start.offset > end.offset {
        return Err(CoreError::CoverageParsing(
            "coverage range ends before it starts".to_string(),
        ));
    }
    Ok(SourceRange { start, end })
}

fn ranges_overlap(left: SourceRange, right: SourceRange) -> bool {
    left.start.offset < right.end.offset && right.start.offset < left.end.offset
}
