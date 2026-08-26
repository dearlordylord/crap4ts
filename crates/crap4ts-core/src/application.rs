//! Application pipeline and quality-gate policy.

use std::{cmp::Ordering, collections::BTreeMap, path::Path};

use crate::{
    coverage::{CoverageAdapter, IstanbulCoverage},
    domain::{
        Complexity, CoreError, Coverage, Diagnostic, DiagnosticCategory, Report, ReportRow,
        SourceFile,
    },
    source,
};

/// Calculate CRAP = CC² × (1 - coverage)³ + CC for measured coverage.
pub fn crap_score(complexity: Complexity, coverage: f64) -> Result<f64, CoreError> {
    if !coverage.is_finite() || !(0.0..=1.0).contains(&coverage) {
        return Err(CoreError::InvalidCoverageFraction(coverage));
    }
    let cc = f64::from(complexity.get());
    Ok(cc * cc * (1.0 - coverage).powi(3) + cc)
}

/// Threshold policy used by the application quality gate.
///
/// Thresholds are resolved by exact normalized project-relative path.  A
/// path-specific value takes precedence over the global value; no glob or
/// basename matching is performed.  Keeping this policy separate from
/// [`crap_score`] means changing the quality bar cannot change the score
/// calculation itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThresholdPolicy {
    global: u32,
    per_path: BTreeMap<crate::domain::ProjectRelativePath, u32>,
}

impl ThresholdPolicy {
    /// Construct a policy with a global threshold and no path overrides.
    pub fn new(global: u32) -> Self {
        Self {
            global,
            per_path: BTreeMap::new(),
        }
    }

    /// Add or replace the exact threshold for a project-relative path.
    pub fn with_path_override(
        mut self,
        path: crate::domain::ProjectRelativePath,
        threshold: u32,
    ) -> Self {
        self.per_path.insert(path, threshold);
        self
    }

    /// Return the global threshold.
    pub const fn global(&self) -> u32 {
        self.global
    }

    /// Return the effective threshold for one normalized project path.
    pub fn threshold_for(&self, path: &crate::domain::ProjectRelativePath) -> u32 {
        self.per_path.get(path).copied().unwrap_or(self.global)
    }

    /// Return whether any measured row strictly exceeds its effective
    /// threshold. Unknown rows are intentionally ignored.
    pub fn gate_breached(&self, report: &Report) -> bool {
        self.gate_breached_rows(&report.rows)
    }

    /// Return whether any row in a slice strictly exceeds its effective
    /// threshold. This form is used while assembling a report, before the
    /// report value itself has been constructed.
    pub fn gate_breached_rows(&self, rows: &[ReportRow]) -> bool {
        rows.iter().any(|row| {
            row.crap
                .is_some_and(|score| score > f64::from(self.threshold_for(&row.path)))
        })
    }

    /// Expose overrides in deterministic path order for callers that need to
    /// inspect or serialize the resolved policy.
    pub fn path_overrides(&self) -> &BTreeMap<crate::domain::ProjectRelativePath, u32> {
        &self.per_path
    }
}

/// Analyze using a current-directory root. This convenience API keeps source
/// and coverage contracts simple for library callers with relative artifacts.
pub fn analyze(
    sources: &[SourceFile],
    coverage_json: &str,
    threshold: u32,
    allow_unknown: bool,
) -> Result<Report, CoreError> {
    analyze_with_root(
        sources,
        coverage_json,
        Path::new("."),
        threshold,
        allow_unknown,
    )
}

/// Analyze source files and an existing Istanbul JSON artifact.
pub fn analyze_with_root(
    sources: &[SourceFile],
    coverage_json: &str,
    root: &Path,
    threshold: u32,
    allow_unknown: bool,
) -> Result<Report, CoreError> {
    analyze_with_policy(
        sources,
        coverage_json,
        root,
        &ThresholdPolicy::new(threshold),
        allow_unknown,
    )
}

/// Analyze source files with a global threshold and deterministic exact path
/// overrides.
pub fn analyze_with_policy(
    sources: &[SourceFile],
    coverage_json: &str,
    root: &Path,
    policy: &ThresholdPolicy,
    allow_unknown: bool,
) -> Result<Report, CoreError> {
    let coverage = IstanbulCoverage::parse(coverage_json, root)?;
    let coverage_adapter: &dyn CoverageAdapter = &coverage;
    let mut units = Vec::new();
    for source_file in sources {
        units.extend(source::analyze_source(
            &source_file.path,
            &source_file.source,
        )?);
        coverage_adapter.validate_for_source(&source_file.path, &source_file.source)?;
    }
    let mut diagnostics = coverage_adapter.validate_attribution(&units, sources)?;

    let mut rows = Vec::with_capacity(units.len());
    for unit in &units {
        let source = sources
            .iter()
            .find(|source| source.path == unit.path)
            .map_or("", |source| source.source.as_str());
        let measured = coverage_adapter.coverage_for(&unit.path, unit, source, &units)?;
        let coverage = measured.unwrap_or_else(|| {
            Coverage::unknown("no matching coverage function or statement evidence")
        });
        if let Coverage::Unknown { reason } = &coverage {
            diagnostics.push(Diagnostic::new(
                DiagnosticCategory::MissingEvidence,
                format!("missing coverage evidence for '{}': {reason}", unit.id),
            ));
            if !allow_unknown {
                return Err(CoreError::MissingEvidence {
                    path: unit.path.to_string(),
                    reason: reason.clone(),
                });
            }
        }
        let crap = coverage.fraction().map(|fraction| {
            crap_score(unit.complexity, fraction).expect("validated coverage fraction")
        });
        rows.push(ReportRow {
            id: unit.id.clone(),
            path: unit.path.clone(),
            name: unit.name.clone(),
            kind: unit.kind,
            range: unit.range,
            body_range: unit.body_range,
            complexity: unit.complexity,
            coverage,
            crap,
        });
    }

    rows.sort_by(compare_rows);
    if policy.gate_breached_rows(&rows) {
        diagnostics.push(Diagnostic::new(
            DiagnosticCategory::ThresholdBreach,
            if policy.path_overrides().is_empty() {
                format!(
                    "one or more CRAP scores exceed the threshold of {}",
                    policy.global()
                )
            } else {
                format!(
                    "one or more CRAP scores exceed the effective threshold (global {})",
                    policy.global()
                )
            },
        ));
    }
    diagnostics.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.message.cmp(&right.message))
    });
    Ok(Report {
        version: crate::domain::REPORT_VERSION,
        threshold: policy.global(),
        rows,
        diagnostics,
    })
}

fn compare_rows(left: &ReportRow, right: &ReportRow) -> Ordering {
    match (left.crap, right.crap) {
        (Some(left_score), Some(right_score)) => right_score
            .partial_cmp(&left_score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.range.start.offset.cmp(&right.range.start.offset))
            .then_with(|| left.range.end.offset.cmp(&right.range.end.offset))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.id.cmp(&right.id)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left
            .path
            .cmp(&right.path)
            .then_with(|| left.range.start.offset.cmp(&right.range.start.offset))
            .then_with(|| left.range.end.offset.cmp(&right.range.end.offset))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.id.cmp(&right.id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, crap: Option<f64>) -> ReportRow {
        let path = crate::domain::ProjectRelativePath::new(path).unwrap();
        let range = crate::domain::SourceRange {
            start: crate::domain::SourcePosition {
                offset: 0,
                line: 1,
                column: 0,
            },
            end: crate::domain::SourcePosition {
                offset: 1,
                line: 1,
                column: 1,
            },
        };
        ReportRow {
            id: format!("{path}::f"),
            path,
            name: "f".to_string(),
            kind: crate::domain::FunctionKind::FunctionDeclaration,
            range,
            body_range: range,
            complexity: Complexity::one(),
            coverage: crap.map_or_else(
                || Coverage::unknown("missing"),
                |_| Coverage::measured(1, 1).unwrap(),
            ),
            crap,
        }
    }

    #[test]
    fn golden_crap_vectors() {
        assert_eq!(crap_score(Complexity::one(), 1.0).unwrap(), 1.0);
        assert_eq!(crap_score(Complexity::one(), 0.0).unwrap(), 2.0);
        assert_eq!(crap_score(Complexity::new(2).unwrap(), 0.5).unwrap(), 2.5);
    }

    #[test]
    fn report_orders_numeric_rows_before_unknown_rows() {
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

    #[test]
    fn threshold_policy_uses_exact_paths_and_strict_comparison() {
        let path = crate::domain::ProjectRelativePath::new("src/a.ts").unwrap();
        let other = crate::domain::ProjectRelativePath::new("src/b.ts").unwrap();
        let policy = ThresholdPolicy::new(8).with_path_override(path.clone(), 1);
        assert_eq!(policy.threshold_for(&path), 1);
        assert_eq!(policy.threshold_for(&other), 8);
        assert!(!policy.gate_breached_rows(&[row("src/a.ts", Some(1.0))]));
        assert!(policy.gate_breached_rows(&[row("src/a.ts", Some(1.000_001))]));
    }
}
