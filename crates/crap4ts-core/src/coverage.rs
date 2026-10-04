//! Coverage adapters and exclusive source-range attribution.

use std::{collections::BTreeMap, path::Path, str::FromStr};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::{
    CoreError, Coverage, CoverageBasis, Diagnostic, DiagnosticCategory, FunctionUnit,
    ProjectRelativePath, SourceFile, SourcePosition, SourceRange,
};

mod lcov;

/// Coverage artifact formats understood by the application boundary.
///
/// The enum deliberately lives in the library rather than the CLI so config
/// loaders and future integrations use the same vocabulary and cannot drift
/// into format-specific branches in scoring or report code.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CoverageFormat {
    /// Istanbul's `coverage-final.json` shape.
    #[serde(rename = "istanbul", alias = "istanbul-json", alias = "json")]
    #[default]
    Istanbul,
    /// The line-oriented LCOV tracefile format.
    #[serde(rename = "lcov", alias = "LCOV")]
    Lcov,
}

impl CoverageFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Istanbul => "istanbul",
            Self::Lcov => "lcov",
        }
    }
}

impl FromStr for CoverageFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "istanbul" | "istanbul-json" | "json" => Ok(Self::Istanbul),
            "lcov" => Ok(Self::Lcov),
            _ => Err(format!(
                "unsupported coverage format '{value}' (expected istanbul or lcov)"
            )),
        }
    }
}

impl std::fmt::Display for CoverageFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Preserve legacy scoring or prefer branches with explicitly reported fallback.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageMetric {
    #[default]
    Legacy,
    Branch,
}

impl FromStr for CoverageMetric {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "legacy" => Ok(Self::Legacy),
            "branch" => Ok(Self::Branch),
            _ => Err("coverage metric must be legacy or branch".to_string()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct IstanbulCoverage {
    files: BTreeMap<ProjectRelativePath, IstanbulFile>,
    metric: CoverageMetric,
}

/// Normalized coverage boundary consumed by scoring.
///
/// Implementations validate their artifact and answer coverage queries using
/// only library-neutral domain values. The application never branches on the
/// concrete report format after construction.
pub trait CoverageAdapter {
    fn uses_branch_metric(&self) -> bool {
        false
    }

    fn validate_for_source(
        &self,
        path: &ProjectRelativePath,
        source: &str,
    ) -> Result<(), CoreError>;

    fn validate_attribution(
        &self,
        units: &[FunctionUnit],
        sources: &[SourceFile],
    ) -> Result<Vec<Diagnostic>, CoreError>;

    fn coverage_for(
        &self,
        path: &ProjectRelativePath,
        unit: &FunctionUnit,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<Option<Coverage>, CoreError>;

    /// Resolve an analysis in bulk. Adapters may share file attribution across
    /// functions; the default preserves independent adapter implementations.
    fn coverage_for_all(
        &self,
        units: &[FunctionUnit],
        sources: &[SourceFile],
    ) -> Result<Vec<Option<Coverage>>, CoreError> {
        units
            .iter()
            .map(|unit| {
                let source = sources
                    .iter()
                    .find(|source| source.path == unit.path)
                    .map_or("", |source| source.source.as_str());
                self.coverage_for(&unit.path, unit, source, units)
            })
            .collect()
    }
}

/// Parse an artifact using the selected format and construct the normalized
/// coverage adapter consumed by [`crate::analyze_with_adapter`].
pub fn make_coverage_adapter(
    format: CoverageFormat,
    input: &str,
    root: &Path,
) -> Result<Box<dyn CoverageAdapter>, CoreError> {
    make_coverage_adapter_with_metric(format, input, root, CoverageMetric::Legacy)
}

pub fn make_coverage_adapter_with_metric(
    format: CoverageFormat,
    input: &str,
    root: &Path,
    metric: CoverageMetric,
) -> Result<Box<dyn CoverageAdapter>, CoreError> {
    match format {
        CoverageFormat::Istanbul => {
            let mut coverage = IstanbulCoverage::parse(input, root)?;
            coverage.metric = metric;
            for file in coverage.files.values_mut() {
                file.metric = metric;
            }
            Ok(Box::new(coverage))
        }
        CoverageFormat::Lcov => Ok(Box::new(lcov::LcovCoverage::parse_with_metric(
            input, root, metric,
        )?)),
    }
}

/// Alias with a noun-first name for callers that prefer a factory-style API.
pub fn coverage_adapter(
    format: CoverageFormat,
    input: &str,
    root: &Path,
) -> Result<Box<dyn CoverageAdapter>, CoreError> {
    make_coverage_adapter(format, input, root)
}

#[derive(Debug)]
struct IstanbulFile {
    function_entries: Vec<IstanbulFunction>,
    statement_entries: Vec<IstanbulStatement>,
    branch_entries: Vec<IstanbulBranch>,
    metric: CoverageMetric,
}

#[derive(Debug)]
struct IstanbulFunction {
    id: String,
    name: String,
    range: IstanbulRange,
    decl: Option<IstanbulRange>,
    hits: Option<u64>,
}

#[derive(Debug)]
struct IstanbulStatement {
    id: String,
    range: IstanbulRange,
    hits: u64,
}

#[derive(Debug)]
struct IstanbulBranch {
    id: String,
    range: IstanbulRange,
    locations: Vec<Option<IstanbulRange>>,
    hits: Vec<u64>,
}

/// The result of joining one Istanbul file to the normalized source units.
///
/// Entries are indexed by their position in the parsed Istanbul maps and hold
/// a global source-unit index. `None` is intentional: coverage artifacts may
/// contain stale, generated, or otherwise unmatchable entries, and those
/// entries must not be guessed into a neighbouring function.
#[derive(Debug)]
struct IstanbulAttribution {
    function_owners: Vec<Option<usize>>,
    statement_owners: Vec<Option<usize>>,
    branch_owners: Vec<Option<usize>>,
    blocked_branches: std::collections::BTreeSet<usize>,
}

/// Istanbul line/column positions use JavaScript UTF-16 code-unit columns.
/// They are converted to the byte-based positions used by Oxc and the domain
/// before any range comparison or attribution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IstanbulPosition {
    line: usize,
    /// `None` is accepted only for Istanbul end positions, where it denotes
    /// the end of the source line. Starts always carry an explicit column.
    column: Option<usize>,
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
                branch_entries: parse_branches(object)?,
                metric: CoverageMetric::Legacy,
            };
            normalized.insert(identity, file);
        }
        Ok(Self {
            files: normalized,
            metric: CoverageMetric::Legacy,
        })
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
    ) -> Result<Vec<Diagnostic>, CoreError> {
        let mut diagnostics = Vec::new();
        for (path, file) in &self.files {
            // Coverage for files outside the selected source set is not part
            // of this report. Ignoring those entries also keeps stale reports
            // from producing unrelated diagnostics.
            if !units.iter().any(|unit| unit.path == *path) {
                continue;
            }
            let source = sources
                .iter()
                .find(|source| source.path == *path)
                .map_or("", |source| source.source.as_str());
            let (attribution, file_diagnostics) = file.attribute(path, source, units)?;
            diagnostics.extend(file_diagnostics);

            // `coverage_for` intentionally remains a small normalized query;
            // source functions without a function-map owner are reported as
            // missing evidence by the application layer. Keeping this fact
            // out of the adapter diagnostics prevents duplicate rows for the
            // same missing-evidence condition.
            debug_assert_eq!(
                attribution.function_owners.len(),
                file.function_entries.len()
            );
        }
        Ok(diagnostics)
    }
}

impl CoverageAdapter for IstanbulCoverage {
    fn uses_branch_metric(&self) -> bool {
        self.metric == CoverageMetric::Branch
    }

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
    ) -> Result<Vec<Diagnostic>, CoreError> {
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

    fn coverage_for_all(
        &self,
        units: &[FunctionUnit],
        sources: &[SourceFile],
    ) -> Result<Vec<Option<Coverage>>, CoreError> {
        let mut measured = vec![None; units.len()];
        for (path, file) in &self.files {
            if !units.iter().any(|unit| unit.path == *path) {
                continue;
            }
            let source = sources
                .iter()
                .find(|source| source.path == *path)
                .map_or("", |source| source.source.as_str());
            let (attribution, _) = file.attribute(path, source, units)?;
            for (index, _) in units
                .iter()
                .enumerate()
                .filter(|(_, unit)| unit.path == *path)
            {
                measured[index] = file.coverage_for_attributed(index, &attribution)?;
            }
        }
        Ok(measured)
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
        for branch in &self.branch_entries {
            source_range_from_istanbul(branch.range, source)?;
            for location in branch.locations.iter().flatten() {
                source_range_from_istanbul(*location, source)?;
            }
        }
        Ok(())
    }

    /// Join all coverage entries to source units in one pass. Building a
    /// complete assignment before scoring is important: querying one source
    /// unit at a time can accidentally let the same fnMap entry or statement
    /// contribute to multiple rows.
    fn attribute(
        &self,
        path: &ProjectRelativePath,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<(IstanbulAttribution, Vec<Diagnostic>), CoreError> {
        let source_units = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.path == *path)
            .collect::<Vec<_>>();
        let function_ranges = self
            .function_entries
            .iter()
            .map(|function| source_range_from_istanbul(function.range, source))
            .collect::<Result<Vec<_>, _>>()?;

        // First assign every function-map entry to one source function. Exact
        // body/range equality has priority over a name-and-overlap fallback;
        // otherwise a broad parent range could steal a nested entry. Within a
        // match class, the smallest source range is the most specific one.
        let mut function_owners = vec![None; self.function_entries.len()];
        let mut unit_functions = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for (function_index, function) in self.function_entries.iter().enumerate() {
            let coverage_range = function_ranges[function_index];
            let mut candidates = source_units
                .iter()
                .filter_map(|(unit_index, unit)| {
                    let exact = coverage_range == unit.body_range
                        || coverage_range == unit.range
                        || body_location_with_delimiters(coverage_range, unit.body_range, source);
                    let location_compatible = range_compatible(coverage_range, unit.range, source)
                        || range_compatible(coverage_range, unit.body_range, source);
                    let named_overlap = !function.name.is_empty()
                        && function.name == unit.name
                        && location_compatible;
                    let anonymous_location =
                        is_istanbul_anonymous(&function.name) && location_compatible;
                    (exact || named_overlap || anonymous_location).then_some((*unit_index, exact))
                })
                .collect::<Vec<_>>();

            // Resolve exact body locations before consulting declarations.
            // Source-map remapping can place a callback's declaration at its
            // enclosing call's method name, outside the callback itself.
            if candidates.iter().any(|(_, exact)| *exact) {
                candidates.retain(|(_, exact)| *exact);
            }

            // Istanbul labels anonymous functions numerically, so the name
            // cannot distinguish a nested arrow from its enclosing arrow.
            // Real artifacts retain a declaration range whose start is the
            // function-like construct's source start. Use that as a second
            // key when a broad null-ended location matches multiple units;
            // without a unique declaration match we fail closed.
            if is_istanbul_anonymous(&function.name) && candidates.len() > 1 {
                let declaration_matches = function
                    .decl
                    .map(|decl| {
                        let declaration = source_range_from_istanbul(decl, source)?;
                        let mut ranked = candidates
                            .iter()
                            .filter_map(|(unit_index, exact)| {
                                declaration_compatibility(declaration, units[*unit_index].range)
                                    .map(|rank| (*unit_index, *exact, rank))
                            })
                            .collect::<Vec<_>>();
                        let strongest = ranked.iter().map(|(_, _, rank)| *rank).max();
                        ranked.retain(|(_, _, rank)| Some(*rank) == strongest);
                        Ok::<_, CoreError>(ranked)
                    })
                    .transpose()?;
                match declaration_matches {
                    Some(matches) if !matches.is_empty() => {
                        candidates = matches
                            .into_iter()
                            .map(|(unit_index, exact, _)| (unit_index, exact))
                            .collect();
                    }
                    _ => {
                        return Err(CoreError::CoverageAttribution(format!(
                            "ambiguous Istanbul anonymous function match for '{}' in '{}'",
                            function.id, path
                        )));
                    }
                }
            }

            let Some((owner, _)) = most_specific_unit(&candidates, units)? else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::CoverageAttribution,
                    format!(
                        "unmatched Istanbul function-map entry '{}' in '{}'",
                        function.id, path
                    ),
                ));
                continue;
            };
            if let Some(previous) = unit_functions.insert(owner, function_index) {
                return Err(CoreError::CoverageAttribution(format!(
                    "ambiguous Istanbul function ownership in '{}': entries '{}' and '{}' both match '{}'",
                    path,
                    self.function_entries[previous].id,
                    function.id,
                    units[owner].id
                )));
            }
            function_owners[function_index] = Some(owner);
        }

        // A source unit with an unmatched child acts as an explicit barrier:
        // statements in that child cannot be attributed to an enclosing
        // matched function. This is deliberately conservative and preserves
        // unknown coverage instead of silently inflating a parent's score.
        let mut statement_owners = vec![None; self.statement_entries.len()];
        for (statement_index, statement) in self.statement_entries.iter().enumerate() {
            let statement_range = source_range_from_istanbul(statement.range, source)?;
            let all_candidates = source_units
                .iter()
                .filter_map(|(unit_index, unit)| {
                    unit_functions
                        .contains_key(unit_index)
                        .then_some(*unit_index)
                        .filter(|_| range_contains(unit.body_range, statement_range))
                })
                .collect::<Vec<_>>();
            let candidates = all_candidates
                .iter()
                .copied()
                .filter(|owner| {
                    !source_units.iter().any(|(unit_index, unit)| {
                        !unit_functions.contains_key(unit_index)
                            && range_contains(unit.range, statement_range)
                            && range_is_strictly_inside(unit.range, units[*owner].range)
                    })
                })
                .collect::<Vec<_>>();
            if !all_candidates.is_empty() && candidates.is_empty() {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::CoverageAttribution,
                    format!(
                        "unmatched Istanbul statement '{}' in '{}' is inside an unmatched source function",
                        statement.id, path
                    ),
                ));
                continue;
            }
            let Some(owner) = most_specific_statement_owner(&candidates, units)? else {
                diagnostics.push(Diagnostic::new(
                    DiagnosticCategory::CoverageAttribution,
                    format!(
                        "unmatched Istanbul statement '{}' in '{}' has no compatible source function",
                        statement.id, path
                    ),
                ));
                continue;
            };
            statement_owners[statement_index] = Some(owner);
        }

        let mut branch_owners = vec![None; self.branch_entries.len()];
        let mut blocked_branches = std::collections::BTreeSet::new();
        if self.metric == CoverageMetric::Branch {
            for (index, branch) in self.branch_entries.iter().enumerate() {
                let range = source_range_from_istanbul(branch.range, source)?;
                let candidates = source_units
                    .iter()
                    .filter_map(|(index, unit)| range_contains(unit.range, range).then_some(*index))
                    .collect::<Vec<_>>();
                let owner = most_specific_statement_owner(&candidates, units)?;
                if let Some(owner) = owner.filter(|owner| unit_functions.contains_key(owner)) {
                    branch_owners[index] = Some(owner);
                } else {
                    blocked_branches.extend(source_units.iter().filter_map(|(index, unit)| {
                        (range.start.offset < unit.range.end.offset
                            && unit.range.start.offset < range.end.offset)
                            .then_some(*index)
                    }));
                    diagnostics.push(Diagnostic::new(DiagnosticCategory::CoverageAttribution,
                        format!("unmatched Istanbul branch '{}' in '{path}' has no compatible source function", branch.id)));
                }
            }
        }

        Ok((
            IstanbulAttribution {
                function_owners,
                statement_owners,
                branch_owners,
                blocked_branches,
            },
            diagnostics,
        ))
    }

    pub(crate) fn coverage_for(
        &self,
        unit: &FunctionUnit,
        source: &str,
        units: &[FunctionUnit],
    ) -> Result<Option<Coverage>, CoreError> {
        let (attribution, _) = self.attribute(&unit.path, source, units)?;
        let unit_index = units
            .iter()
            .position(|candidate| candidate.id == unit.id)
            .ok_or_else(|| {
                CoreError::CoverageAttribution(format!(
                    "source function '{}' is absent from the attribution set",
                    unit.id
                ))
            })?;
        self.coverage_for_attributed(unit_index, &attribution)
    }

    fn coverage_for_attributed(
        &self,
        unit_index: usize,
        attribution: &IstanbulAttribution,
    ) -> Result<Option<Coverage>, CoreError> {
        if self.metric == CoverageMetric::Branch {
            if attribution.blocked_branches.contains(&unit_index) {
                return Ok(None);
            }
            let hits = attribution
                .branch_owners
                .iter()
                .zip(&self.branch_entries)
                .filter(|(owner, _)| **owner == Some(unit_index))
                .flat_map(|(_, branch)| branch.hits.iter())
                .collect::<Vec<_>>();
            if !hits.is_empty() {
                return Coverage::measured(
                    hits.iter().filter(|hit| ***hit > 0).count() as u64,
                    hits.len() as u64,
                )
                .map(|coverage| Some(coverage.with_basis(CoverageBasis::Branch)));
            }
        }
        let owned = attribution
            .statement_owners
            .iter()
            .zip(&self.statement_entries)
            .filter_map(|(owner, statement)| (*owner == Some(unit_index)).then_some(statement.hits))
            .collect::<Vec<_>>();
        if !owned.is_empty() {
            let total = owned.len() as u64;
            let covered = owned.iter().filter(|hits| **hits > 0).count() as u64;
            return Coverage::measured(covered, total).map(|coverage| {
                Some(if self.metric == CoverageMetric::Branch {
                    coverage.with_basis(CoverageBasis::Statement)
                } else {
                    coverage
                })
            });
        }
        let function = attribution
            .function_owners
            .iter()
            .enumerate()
            .find_map(|(index, owner)| (*owner == Some(unit_index)).then_some(index));
        Ok(function
            .and_then(|index| self.function_entries[index].hits)
            .and_then(|hits| Coverage::measured(u64::from(hits > 0), 1).ok())
            .map(|coverage| {
                if self.metric == CoverageMetric::Branch {
                    coverage.with_basis(CoverageBasis::Function)
                } else {
                    coverage
                }
            }))
    }
}

/// Match Istanbul's declaration range without assuming its start is the
/// exact Oxc function span. Constructors can include indentation, decorators,
/// or a method key before the span Oxc reports. A declaration start inside a
/// source function's start or a short prefix around that start is stronger
/// than merely lying somewhere inside an enclosing function's body.
fn declaration_compatibility(declaration: SourceRange, unit: SourceRange) -> Option<u8> {
    if declaration.start == unit.start && declaration.end.offset <= unit.end.offset {
        Some(3)
    } else if declaration.start.offset <= unit.start.offset
        && declaration.end.offset >= unit.start.offset
    {
        Some(if declaration.end.offset < unit.end.offset {
            2
        } else {
            0
        })
    } else if range_contains(unit, declaration) {
        Some(1)
    } else {
        None
    }
}

/// Select the unique smallest source range. A deterministic secondary sort
/// would still be a guess when ranges tie, so ties are rejected explicitly.
fn most_specific_unit(
    candidates: &[(usize, bool)],
    units: &[FunctionUnit],
) -> Result<Option<(usize, bool)>, CoreError> {
    let Some(minimum) = candidates
        .iter()
        .map(|(index, _)| units[*index].range.size())
        .min()
    else {
        return Ok(None);
    };
    let matches = candidates
        .iter()
        .filter(|(index, _)| units[*index].range.size() == minimum)
        .copied()
        .collect::<Vec<_>>();
    if matches.len() > 1
        || candidates.iter().any(|(index, _)| {
            *index != matches[0].0
                && !range_is_strictly_inside(units[matches[0].0].range, units[*index].range)
        })
    {
        let other = candidates
            .iter()
            .find(|(index, _)| *index != matches[0].0)
            .map(|(index, _)| *index)
            .unwrap_or(matches[0].0);
        return Err(CoreError::CoverageAttribution(format!(
            "ambiguous Istanbul function match among source functions '{}' and '{}'",
            units[matches[0].0].id, units[other].id
        )));
    }
    Ok(matches.into_iter().next())
}

fn most_specific_statement_owner(
    candidates: &[usize],
    units: &[FunctionUnit],
) -> Result<Option<usize>, CoreError> {
    let Some(minimum) = candidates
        .iter()
        .map(|index| units[*index].body_range.size())
        .min()
    else {
        return Ok(None);
    };
    let matches = candidates
        .iter()
        .copied()
        .filter(|index| units[*index].body_range.size() == minimum)
        .collect::<Vec<_>>();
    if matches.len() > 1
        || candidates.iter().any(|index| {
            *index != matches[0]
                && !range_is_strictly_inside(units[matches[0]].body_range, units[*index].body_range)
        })
    {
        let other = candidates
            .iter()
            .find(|index| **index != matches[0])
            .copied()
            .unwrap_or(matches[0]);
        return Err(CoreError::CoverageAttribution(format!(
            "ambiguous statement ownership among source functions '{}' and '{}'",
            units[matches[0]].id, units[other].id
        )));
    }
    Ok(matches.into_iter().next())
}

fn range_is_strictly_inside(inner: SourceRange, outer: SourceRange) -> bool {
    outer.start.offset <= inner.start.offset
        && inner.end.offset <= outer.end.offset
        && (outer.start.offset < inner.start.offset || inner.end.offset < outer.end.offset)
}

/// Remapped expression bodies can include parentheses around an object or a
/// call's trailing comma. Only same-line syntactic wrappers retain the body's
/// identity; containing a body somewhere is not sufficient.
fn body_location_with_delimiters(coverage: SourceRange, body: SourceRange, source: &str) -> bool {
    coverage.start.line == body.start.line
        && coverage.end.line == body.end.line
        && source
            .get(
                coverage.start.offset.min(body.start.offset)
                    ..coverage.start.offset.max(body.start.offset),
            )
            .is_some_and(|prefix| {
                prefix
                    .chars()
                    .all(|character| character.is_whitespace() || character == '(')
            })
        && source
            .get(body.end.offset.min(coverage.end.offset)..body.end.offset.max(coverage.end.offset))
            .is_some_and(|suffix| {
                suffix.chars().all(|character| {
                    character.is_whitespace() || matches!(character, ';' | ',' | ')' | ']' | '}')
                })
            })
}

/// Istanbul commonly represents an end column as null, meaning the end of
/// that source line. That envelope can include a closing delimiter or
/// semicolon immediately after Oxc's function span. Preserve containment as
/// the primary rule, while accepting only this harmless same-line suffix;
/// arbitrary partial overlaps remain incompatible.
fn range_compatible(coverage: SourceRange, unit: SourceRange, source: &str) -> bool {
    // A location beginning exactly at the exclusive end is only a separator
    // (or stale metadata), never evidence for this function. This guard must
    // precede both ordinary containment and the delimiter-envelope fallback.
    if coverage.start.offset >= unit.end.offset || coverage.end.offset <= unit.start.offset {
        return false;
    }
    if ranges_contained(coverage, unit) {
        return true;
    }
    if coverage.start.offset < unit.start.offset
        || coverage.start.offset > unit.end.offset
        || coverage.end.line != unit.end.line
        || coverage.end.offset < unit.end.offset
    {
        return false;
    }
    source
        .get(unit.end.offset..coverage.end.offset)
        .is_some_and(|suffix| {
            suffix.chars().all(|character| {
                character.is_whitespace() || matches!(character, ';' | ',' | ')' | ']' | '}')
            })
        })
}

fn ranges_contained(left: SourceRange, right: SourceRange) -> bool {
    range_contains(left, right) || range_contains(right, left)
}

fn range_contains(outer: SourceRange, inner: SourceRange) -> bool {
    outer.start.offset <= inner.start.offset && inner.end.offset <= outer.end.offset
}

fn is_istanbul_anonymous(name: &str) -> bool {
    name == "(anonymous)" || name.starts_with("(anonymous_")
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
    if let Some(counts) = &counts {
        if counts.keys().any(|id| !map.contains_key(id)) {
            return Err(CoreError::CoverageParsing(
                "f contains a function count without a fnMap entry".to_string(),
            ));
        }
    }
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
        let decl = entry
            .get("decl")
            .map(|value| parse_range(value, &format!("fnMap entry '{id}' declaration")))
            .transpose()?;
        let hits = counts
            .as_ref()
            .map(|counts| {
                counts.get(id).copied().ok_or_else(|| {
                    CoreError::CoverageParsing(format!("f is missing count for function '{id}'"))
                })
            })
            .transpose()?;
        functions.push(IstanbulFunction {
            id: id.clone(),
            name,
            range,
            decl,
            hits,
        });
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
        statements.push(IstanbulStatement {
            id: id.clone(),
            range,
            hits,
        });
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

// Istanbul represents the implicit else of an if without an else with
// empty positions. Its outcome still belongs to the decision's explicit loc.
fn parse_branch_location(value: &Value) -> Result<Option<IstanbulRange>, CoreError> {
    let empty = |key| {
        value
            .get(key)
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty)
    };
    if empty("start") && empty("end") {
        return Ok(None);
    }
    parse_range(value, "branch location").map(Some)
}

fn parse_branches(object: &Map<String, Value>) -> Result<Vec<IstanbulBranch>, CoreError> {
    let Some(map) = object.get("branchMap") else {
        if object
            .get("b")
            .and_then(Value::as_object)
            .is_some_and(|map| !map.is_empty())
        {
            return Err(CoreError::CoverageParsing(
                "b has counts without branchMap".to_string(),
            ));
        }
        return Ok(Vec::new());
    };
    let map = map
        .as_object()
        .ok_or_else(|| CoreError::CoverageParsing("branchMap must be an object".to_string()))?;
    let counts = object.get("b").and_then(Value::as_object);
    let mut branches = Vec::new();
    for (id, entry) in map {
        let range = parse_range(
            entry
                .get("loc")
                .ok_or_else(|| CoreError::CoverageParsing(format!("branch '{id}' has no loc")))?,
            "branch",
        )?;
        let locations = entry
            .get("locations")
            .and_then(Value::as_array)
            .ok_or_else(|| CoreError::CoverageParsing(format!("branch '{id}' has no locations")))?
            .iter()
            .map(parse_branch_location)
            .collect::<Result<Vec<_>, _>>()?;
        let hits = counts
            .and_then(|map| map.get(id))
            .and_then(Value::as_array)
            .ok_or_else(|| CoreError::CoverageParsing(format!("branch '{id}' has no counts")))?
            .iter()
            .map(|value| {
                value.as_u64().ok_or_else(|| {
                    CoreError::CoverageParsing(format!("branch '{id}' has invalid count"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if hits.is_empty() || hits.len() != locations.len() {
            return Err(CoreError::CoverageParsing(format!(
                "branch '{id}' count length does not match locations"
            )));
        }
        branches.push(IstanbulBranch {
            id: id.clone(),
            range,
            locations,
            hits,
        });
    }
    if counts.is_some_and(|counts| counts.keys().any(|id| !map.contains_key(id))) {
        return Err(CoreError::CoverageParsing(
            "b contains an unknown branch id".to_string(),
        ));
    }
    Ok(branches)
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
        false,
    )?;
    let end = parse_position(
        object
            .get("end")
            .ok_or_else(|| CoreError::CoverageParsing(format!("{context} has no end")))?,
        context,
        true,
    )?;
    if start.line > end.line
        || (start.line == end.line
            && end
                .column
                .is_some_and(|end_column| start.column.unwrap_or_default() >= end_column))
    {
        return Err(CoreError::CoverageParsing(format!(
            "{context} range ends before it starts"
        )));
    }
    Ok(IstanbulRange { start, end })
}

fn parse_position(
    value: &Value,
    context: &str,
    allow_null_column: bool,
) -> Result<IstanbulPosition, CoreError> {
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
    let column = match object.get("column") {
        Some(value) if value.is_null() && allow_null_column => None,
        Some(value) => Some(
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| {
                    CoreError::CoverageParsing(format!(
                        "{context} column must be a nonnegative integer"
                    ))
                })?,
        ),
        None => {
            return Err(CoreError::CoverageParsing(format!(
                "{context} column must be a nonnegative integer"
            )))
        }
    };
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
    // Istanbul locations use JavaScript string columns (UTF-16 code units),
    // while Oxc spans and the normalized domain use UTF-8 byte offsets. Walk
    // Unicode scalar boundaries instead of adding the raw column to the byte
    // offset. A column in the middle of a surrogate pair is rejected rather
    // than producing a range that points into a non-character byte sequence.
    let line = &source[line_start..content_end];
    let target_column = position
        .column
        .unwrap_or_else(|| line.encode_utf16().count());
    let mut utf16_column = 0;
    let mut byte_column = 0;
    for character in line.chars() {
        if target_column == utf16_column {
            return Ok(SourcePosition {
                offset: line_start + byte_column,
                line: position.line,
                column: byte_column,
            });
        }
        let next_utf16_column = utf16_column + character.len_utf16();
        if target_column < next_utf16_column {
            return Err(CoreError::CoverageParsing(format!(
                "coverage column {} falls inside a UTF-16 character on source line {}",
                target_column, position.line
            )));
        }
        utf16_column = next_utf16_column;
        byte_column += character.len_utf8();
    }
    if target_column == utf16_column {
        return Ok(SourcePosition {
            offset: line_start + byte_column,
            line: position.line,
            column: byte_column,
        });
    }
    Err(CoreError::CoverageParsing(format!(
        "coverage column {} is outside source line {}",
        target_column, position.line
    )))
}

fn source_range_from_istanbul(
    range: IstanbulRange,
    source: &str,
) -> Result<SourceRange, CoreError> {
    let start = source_position_from_istanbul(range.start, source)?;
    let end = source_position_from_istanbul(range.end, source)?;
    if start.offset >= end.offset {
        return Err(CoreError::CoverageParsing(
            "coverage range ends before it starts".to_string(),
        ));
    }
    Ok(SourceRange { start, end })
}
