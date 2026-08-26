//! LCOV line-coverage adapter.
//!
//! LCOV deliberately remains line based here. It has no source columns and
//! its optional function records do not describe function end ranges, so a
//! line can be assigned only when the normalized source ranges make ownership
//! unambiguous. Guessing between same-line functions would turn unavailable
//! evidence into a misleading score; those lines are therefore retained as
//! unknown with a structured attribution diagnostic.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::domain::{
    CoreError, Coverage, Diagnostic, DiagnosticCategory, FunctionUnit, ProjectRelativePath,
    SourceFile, SourceRange,
};

use super::CoverageAdapter;

#[derive(Debug)]
pub(super) struct LcovCoverage {
    files: BTreeMap<ProjectRelativePath, LcovFile>,
}

#[derive(Debug, Default)]
struct LcovFile {
    /// LCOV's `DA` records are the normalized line evidence. The key is the
    /// one-based source line and the value is its execution count.
    lines: BTreeMap<usize, u64>,
    function_lines: Vec<(usize, String)>,
    function_hits: Vec<(u64, String)>,
    function_found: Option<u64>,
    function_hit: Option<u64>,
    lines_found: Option<u64>,
    lines_hit: Option<u64>,
}

#[derive(Debug)]
struct LcovAttribution {
    /// Hits indexed by the global source-unit index supplied by the
    /// application. A unit with no entry has no measurable LCOV evidence.
    unit_hits: BTreeMap<usize, Vec<u64>>,
    /// Units touched by an ambiguous line are explicitly blocked from
    /// becoming measured by another, unrelated line.
    ambiguous_units: BTreeSet<usize>,
}

impl LcovCoverage {
    pub(super) fn parse(input: &str, root: &Path) -> Result<Self, CoreError> {
        let mut files = BTreeMap::new();
        let mut current: Option<(String, LcovFile)> = None;

        for (line_number, raw_line) in input.lines().enumerate() {
            let line_number = line_number + 1;
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let line = line.strip_prefix('\u{feff}').unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            if line == "end_of_record" {
                let (raw_path, file) = current.take().ok_or_else(|| {
                    parsing_error(line_number, "end_of_record has no preceding SF record")
                })?;
                let path = ProjectRelativePath::from_artifact_path(&raw_path, root)
                    .map_err(|error| parsing_error(line_number, error.to_string()))?;
                file.validate_summaries(&path)?;
                if files.insert(path.clone(), file).is_some() {
                    return Err(CoreError::AmbiguousCoverageFile(path.to_string()));
                }
                continue;
            }

            let (tag, value) = line
                .split_once(':')
                .ok_or_else(|| parsing_error(line_number, "record must contain a ':' separator"))?;
            match tag {
                // Test names and summary fields are useful metadata for LCOV
                // consumers, but line coverage is normalized from DA below.
                "TN" => {}
                "SF" => {
                    if current.is_some() {
                        return Err(parsing_error(
                            line_number,
                            "SF starts a new record before end_of_record",
                        ));
                    }
                    if value.is_empty() {
                        return Err(parsing_error(line_number, "SF path must not be empty"));
                    }
                    current = Some((value.to_string(), LcovFile::default()));
                }
                "FN" => {
                    let (function_line, function_name) =
                        value.split_once(',').ok_or_else(|| {
                            parsing_error(line_number, "FN must contain a line and function name")
                        })?;
                    let function_line = parse_positive_line(function_line, "FN line", line_number)?;
                    let (_, file) = current.as_mut().ok_or_else(|| {
                        parsing_error(line_number, "FN appears before an SF record")
                    })?;
                    file.function_lines
                        .push((function_line, function_name.to_string()));
                    // Function names are intentionally not used for
                    // attribution: LCOV does not provide an end range and
                    // names can collide within one file.
                }
                "FNDA" => {
                    let (hits, function_name) = value.split_once(',').ok_or_else(|| {
                        parsing_error(
                            line_number,
                            "FNDA must contain a hit count and function name",
                        )
                    })?;
                    let hits = parse_count(hits, "FNDA hit count", line_number)?;
                    let (_, file) = current.as_mut().ok_or_else(|| {
                        parsing_error(line_number, "FNDA appears before an SF record")
                    })?;
                    file.function_hits.push((hits, function_name.to_string()));
                }
                "DA" => {
                    let (line_value, rest) = value.split_once(',').ok_or_else(|| {
                        parsing_error(line_number, "DA must contain a source line and hit count")
                    })?;
                    let (hits, _) = rest.split_once(',').map_or((rest, ""), |parts| parts);
                    let source_line = parse_positive_line(line_value, "DA line", line_number)?;
                    let hits = parse_count(hits, "DA hit count", line_number)?;
                    let (_, file) = current.as_mut().ok_or_else(|| {
                        parsing_error(line_number, "DA appears before an SF record")
                    })?;
                    if file.lines.insert(source_line, hits).is_some() {
                        return Err(parsing_error(
                            line_number,
                            format!("duplicate DA record for source line {source_line}"),
                        ));
                    }
                }
                "FNF" | "FNH" | "LF" | "LH" => {
                    let count = parse_count(value, tag, line_number)?;
                    let (_, file) = current.as_mut().ok_or_else(|| {
                        parsing_error(line_number, format!("{tag} appears before an SF record"))
                    })?;
                    let slot = match tag {
                        "FNF" => &mut file.function_found,
                        "FNH" => &mut file.function_hit,
                        "LF" => &mut file.lines_found,
                        "LH" => &mut file.lines_hit,
                        _ => unreachable!(),
                    };
                    if slot.replace(count).is_some() {
                        return Err(parsing_error(
                            line_number,
                            format!("duplicate {tag} summary record"),
                        ));
                    }
                }
                "BRDA" => {
                    parse_branch_record(value, line_number)?;
                    require_record(&current, line_number, "BRDA")?;
                }
                "BRF" | "BRH" => {
                    parse_count(value, tag, line_number)?;
                    require_record(&current, line_number, tag)?;
                }
                _ => {
                    return Err(parsing_error(
                        line_number,
                        format!("unsupported LCOV record '{tag}'"),
                    ));
                }
            }
        }

        if current.is_some() {
            return Err(parsing_error(
                input.lines().count().max(1),
                "LCOV record is missing end_of_record",
            ));
        }
        Ok(Self { files })
    }

    pub(super) fn validate_for_source(
        &self,
        path: &ProjectRelativePath,
        source: &str,
    ) -> Result<(), CoreError> {
        let Some(file) = self.files.get(path) else {
            return Ok(());
        };
        let line_count = source_line_count(source);
        if let Some(line) = file.lines.keys().find(|line| **line > line_count) {
            return Err(CoreError::CoverageParsing(format!(
                "LCOV line {line} for '{path}' is outside the source ({line_count} lines)"
            )));
        }
        Ok(())
    }

    pub(super) fn validate_attribution(
        &self,
        units: &[FunctionUnit],
        sources: &[SourceFile],
    ) -> Result<Vec<Diagnostic>, CoreError> {
        let mut diagnostics = Vec::new();
        for (path, file) in &self.files {
            if !units.iter().any(|unit| unit.path == *path) {
                continue;
            }
            let source = sources
                .iter()
                .find(|source| source.path == *path)
                .map_or("", |source| source.source.as_str());
            let (_, file_diagnostics) = file.attribute(path, source, units)?;
            diagnostics.extend(file_diagnostics);
        }
        Ok(diagnostics)
    }

    pub(super) fn coverage_for(
        &self,
        path: &ProjectRelativePath,
        unit: &FunctionUnit,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<Option<Coverage>, CoreError> {
        let Some(file) = self.files.get(path) else {
            return Ok(None);
        };
        let (attribution, _) = file.attribute(path, source, units)?;
        let unit_index = units
            .iter()
            .position(|candidate| candidate.id == unit.id)
            .ok_or_else(|| {
                CoreError::CoverageAttribution(format!(
                    "source function '{}' is absent from the attribution set",
                    unit.id
                ))
            })?;
        if attribution.ambiguous_units.contains(&unit_index) {
            return Ok(None);
        }
        let Some(hits) = attribution.unit_hits.get(&unit_index) else {
            return Ok(None);
        };
        let total = hits.len() as u64;
        let covered = hits.iter().filter(|hits| **hits > 0).count() as u64;
        Coverage::measured(covered, total).map(Some)
    }
}

impl CoverageAdapter for LcovCoverage {
    fn validate_for_source(
        &self,
        path: &ProjectRelativePath,
        source: &str,
    ) -> Result<(), CoreError> {
        Self::validate_for_source(self, path, source)
    }

    fn validate_attribution(
        &self,
        units: &[FunctionUnit],
        sources: &[SourceFile],
    ) -> Result<Vec<Diagnostic>, CoreError> {
        Self::validate_attribution(self, units, sources)
    }

    fn coverage_for(
        &self,
        path: &ProjectRelativePath,
        unit: &FunctionUnit,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<Option<Coverage>, CoreError> {
        Self::coverage_for(self, path, unit, source, units)
    }
}

impl LcovFile {
    fn validate_summaries(&self, path: &ProjectRelativePath) -> Result<(), CoreError> {
        let function_count = self.function_lines.len() as u64;
        if self
            .function_found
            .is_some_and(|found| found != function_count)
        {
            return Err(CoreError::CoverageParsing(format!(
                "LCOV function summary for '{path}' does not match FN records"
            )));
        }
        if self.function_hits.len() != self.function_lines.len() {
            return Err(CoreError::CoverageParsing(format!(
                "LCOV FNDA record count for '{path}' does not match FN records"
            )));
        }
        let mut function_names = BTreeMap::<&str, usize>::new();
        for (_, name) in &self.function_lines {
            *function_names.entry(name.as_str()).or_default() += 1;
        }
        for (_, name) in &self.function_hits {
            let Some(remaining) = function_names.get_mut(name.as_str()) else {
                return Err(CoreError::CoverageParsing(format!(
                    "LCOV FNDA function '{name}' in '{path}' has no matching FN record"
                )));
            };
            if *remaining == 0 {
                return Err(CoreError::CoverageParsing(format!(
                    "LCOV FNDA function '{name}' in '{path}' occurs too many times"
                )));
            }
            *remaining -= 1;
        }
        if function_names.values().any(|remaining| *remaining != 0) {
            return Err(CoreError::CoverageParsing(format!(
                "LCOV FN record in '{path}' has no matching FNDA record"
            )));
        }
        let function_hit_count = self
            .function_hits
            .iter()
            .filter(|(hits, _)| *hits > 0)
            .count() as u64;
        if self
            .function_hit
            .is_some_and(|hit| hit != function_hit_count)
        {
            return Err(CoreError::CoverageParsing(format!(
                "LCOV function-hit summary for '{path}' does not match FNDA records"
            )));
        }
        let line_count = self.lines.len() as u64;
        if self.lines_found.is_some_and(|found| found != line_count) {
            return Err(CoreError::CoverageParsing(format!(
                "LCOV line summary for '{path}' does not match DA records"
            )));
        }
        let line_hit_count = self.lines.values().filter(|hits| **hits > 0).count() as u64;
        if self.lines_hit.is_some_and(|hit| hit != line_hit_count) {
            return Err(CoreError::CoverageParsing(format!(
                "LCOV line-hit summary for '{path}' does not match DA records"
            )));
        }
        Ok(())
    }

    fn attribute(
        &self,
        path: &ProjectRelativePath,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<(LcovAttribution, Vec<Diagnostic>), CoreError> {
        let source_units = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.path == *path)
            .collect::<Vec<_>>();
        let mut attribution = LcovAttribution {
            unit_hits: BTreeMap::new(),
            ambiguous_units: BTreeSet::new(),
        };
        let mut diagnostics = Vec::new();

        for (line, hits) in &self.lines {
            let candidates = source_units
                .iter()
                // LCOV DA records identify source lines, so include the full
                // normalized function range (including a multiline
                // signature) when looking for an owner. Nested-body
                // exclusivity is still decided from body ranges below.
                .filter(|(_, unit)| line_in_range(*line, unit.range))
                .map(|(index, _)| *index)
                .collect::<Vec<_>>();
            match line_owner(*line, &candidates, units, source) {
                LineOwner::Unique(owner) => {
                    attribution.unit_hits.entry(owner).or_default().push(*hits);
                }
                LineOwner::Ambiguous => {
                    attribution
                        .ambiguous_units
                        .extend(candidates.iter().copied());
                    diagnostics.push(ambiguous_line_diagnostic(path, *line, &candidates, units));
                }
                LineOwner::None => {
                    diagnostics.push(Diagnostic::new(
                        DiagnosticCategory::CoverageAttribution,
                        format!(
                            "unmatched LCOV line {line} in '{path}' has no compatible source function"
                        ),
                    ));
                }
            }
        }
        Ok((attribution, diagnostics))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LineOwner {
    None,
    Unique(usize),
    Ambiguous,
}

fn line_owner(
    line: usize,
    candidates: &[usize],
    units: &[FunctionUnit],
    source: &str,
) -> LineOwner {
    let Some((&only, rest)) = candidates.split_first() else {
        return LineOwner::None;
    };
    if rest.is_empty() {
        return if line_has_executable_outside_range(source, line, &units[only]) {
            LineOwner::Ambiguous
        } else {
            LineOwner::Unique(only)
        };
    }

    // A line nested entirely inside one source body can be attributed to the
    // innermost body, preserving the same exclusive ownership guarantee as
    // Istanbul. Equal or sibling ranges remain ambiguous.
    let mut ranked = candidates.to_vec();
    ranked.sort_by_key(|index| units[*index].body_range.size());
    let candidate = ranked[0];
    let tied = ranked
        .iter()
        .skip(1)
        .any(|index| units[*index].body_range.size() == units[candidate].body_range.size());
    let nested_in_all = ranked.iter().skip(1).all(|index| {
        range_is_strictly_inside(units[candidate].body_range, units[*index].body_range)
    });
    if tied || !nested_in_all {
        return LineOwner::Ambiguous;
    }

    // LCOV covers a whole line. If the enclosing body has any non-whitespace
    // source on that line outside the inner body, the line could represent
    // either function and cannot be safely assigned.
    if ranked.iter().skip(1).any(|outer| {
        line_has_non_whitespace_outside(
            source,
            line,
            units[candidate].body_range,
            units[*outer].body_range,
        )
    }) {
        LineOwner::Ambiguous
    } else {
        LineOwner::Unique(candidate)
    }
}

fn ambiguous_line_diagnostic(
    path: &ProjectRelativePath,
    line: usize,
    candidates: &[usize],
    units: &[FunctionUnit],
) -> Diagnostic {
    let names = candidates
        .iter()
        .map(|index| units[*index].id.as_str())
        .collect::<Vec<_>>();
    Diagnostic::new(
        DiagnosticCategory::CoverageAttribution,
        format!(
            "ambiguous LCOV attribution for line {line} in '{path}': line-only coverage cannot distinguish source functions {}",
            quoted_names(&names)
        ),
    )
}

fn quoted_names(names: &[&str]) -> String {
    match names {
        [] => "<none>".to_string(),
        [only] => format!("'{only}'"),
        [first, second] => format!("'{first}' and '{second}'"),
        [first, rest @ ..] => format!(
            "'{}' and {}",
            first,
            rest.iter()
                .map(|name| format!("'{name}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn line_in_range(line: usize, range: SourceRange) -> bool {
    range.start.line <= line && line <= range.end.line && range.start.offset < range.end.offset
}

fn range_is_strictly_inside(inner: SourceRange, outer: SourceRange) -> bool {
    outer.start.offset <= inner.start.offset
        && inner.end.offset <= outer.end.offset
        && (outer.start.offset < inner.start.offset || inner.end.offset < outer.end.offset)
}

fn line_has_non_whitespace_outside(
    source: &str,
    line: usize,
    inner: SourceRange,
    outer: SourceRange,
) -> bool {
    let Some((line_start, line_end)) = source_line_span(source, line) else {
        return true;
    };
    let outer_start = outer.start.offset.max(line_start).min(line_end);
    let outer_end = outer.end.offset.max(line_start).min(line_end);
    if outer_start >= outer_end {
        return false;
    }
    let inner_start = inner.start.offset.max(line_start).min(line_end);
    let inner_end = inner.end.offset.max(line_start).min(line_end);
    let before = &source[outer_start..inner_start.min(outer_end)];
    let after_start = inner_end.max(outer_start).min(outer_end);
    let after = &source[after_start..outer_end];
    before.chars().any(|character| !character.is_whitespace())
        || after.chars().any(|character| !character.is_whitespace())
}

fn line_has_executable_outside_range(source: &str, line: usize, unit: &FunctionUnit) -> bool {
    let Some((line_start, line_end)) = source_line_span(source, line) else {
        return true;
    };
    let range_start = unit.range.start.offset.max(line_start).min(line_end);
    let range_end = unit.range.end.offset.max(line_start).min(line_end);
    let before = &source[line_start..range_start];
    let after = &source[range_end..line_end];
    executable_prefix(before, unit) || executable_suffix(after, unit)
}

fn executable_prefix(value: &str, unit: &FunctionUnit) -> bool {
    let value = value.split_once("//").map_or(value, |(prefix, _)| prefix);
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    if value.contains(';') || contains_control_keyword(value) {
        return true;
    }
    if value.ends_with(['=', ':', ',']) {
        return false;
    }
    if matches!(
        unit.kind,
        crate::domain::FunctionKind::Method
            | crate::domain::FunctionKind::Getter
            | crate::domain::FunctionKind::Setter
            | crate::domain::FunctionKind::Constructor
    ) && value.contains('{')
        && !value.contains('}')
        && !value.contains('=')
    {
        // A class/object header and the method/accessor key belong to this
        // declaration envelope. A control-flow token or prior closed block
        // was rejected above/below, so it cannot be mistaken for a header.
        return false;
    }
    if value.ends_with(['(', '[', '{']) {
        let prefix = value[..value.len() - 1].trim_end();
        return prefix
            .chars()
            .last()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_');
    }
    let tokens = value.split_whitespace().collect::<Vec<_>>();
    !tokens.iter().all(|token| {
        matches!(
            *token,
            "export"
                | "default"
                | "declare"
                | "async"
                | "public"
                | "private"
                | "protected"
                | "static"
                | "abstract"
                | "readonly"
                | "override"
        ) || token.starts_with('@')
    })
}

fn executable_suffix(value: &str, unit: &FunctionUnit) -> bool {
    let value = value.split_once("//").map_or(value, |(prefix, _)| prefix);
    let value = value.trim();
    if value.is_empty() || value == ";" {
        return false;
    }
    let value = value.strip_prefix(';').map_or(value, str::trim_start);
    if value.is_empty() {
        return false;
    }
    if matches!(
        unit.kind,
        crate::domain::FunctionKind::Method
            | crate::domain::FunctionKind::Getter
            | crate::domain::FunctionKind::Setter
            | crate::domain::FunctionKind::Constructor
    ) && value
        .chars()
        .all(|character| matches!(character, '}' | ';' | ','))
    {
        return false;
    }
    true
}

fn contains_control_keyword(value: &str) -> bool {
    value
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|token| {
            matches!(
                token,
                "if" | "for"
                    | "while"
                    | "do"
                    | "switch"
                    | "catch"
                    | "try"
                    | "return"
                    | "throw"
                    | "yield"
                    | "await"
                    | "&&"
                    | "||"
                    | "??"
            )
        })
}

fn source_line_count(source: &str) -> usize {
    source.bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn source_line_span(source: &str, line: usize) -> Option<(usize, usize)> {
    if line == 0 {
        return None;
    }
    let mut current_line = 1;
    let mut start = 0;
    for (offset, byte) in source.bytes().enumerate() {
        if current_line == line && byte == b'\n' {
            return Some((start, offset));
        }
        if byte == b'\n' {
            current_line += 1;
            start = offset + 1;
        }
    }
    (current_line == line).then_some((start, source.len()))
}

fn require_record<T>(
    current: &Option<(String, T)>,
    line: usize,
    tag: &str,
) -> Result<(), CoreError> {
    current
        .is_some()
        .then_some(())
        .ok_or_else(|| parsing_error(line, format!("{tag} appears before an SF record")))
}

fn parse_positive_line(value: &str, label: &str, line: usize) -> Result<usize, CoreError> {
    let parsed = parse_count(value, label, line)?;
    usize::try_from(parsed).map_err(|_| parsing_error(line, format!("{label} is too large")))
}

fn parse_branch_record(value: &str, line: usize) -> Result<(), CoreError> {
    let fields = value.split(',').collect::<Vec<_>>();
    if fields.len() != 4 {
        return Err(parsing_error(
            line,
            "BRDA must contain line, block, branch, and taken fields",
        ));
    }
    parse_positive_line(fields[0], "BRDA line", line)?;
    if fields[1].is_empty() || fields[2].is_empty() {
        return Err(parsing_error(
            line,
            "BRDA block and branch fields must not be empty",
        ));
    }
    if fields[3] != "-" {
        parse_count(fields[3], "BRDA taken count", line)?;
    }
    Ok(())
}

fn parse_count(value: &str, label: &str, line: usize) -> Result<u64, CoreError> {
    value
        .parse::<u64>()
        .map_err(|_| parsing_error(line, format!("{label} must be a nonnegative integer")))
        .and_then(|value| {
            if value == 0 && label.ends_with("line") {
                Err(parsing_error(
                    line,
                    format!("{label} must be greater than zero"),
                ))
            } else {
                Ok(value)
            }
        })
}

fn parsing_error(line: usize, message: impl Into<String>) -> CoreError {
    CoreError::CoverageParsing(format!("LCOV line {line}: {}", message.into()))
}
