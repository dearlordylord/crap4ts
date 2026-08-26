//! Application pipeline and quality-gate policy.

use std::{cmp::Ordering, collections::BTreeMap, path::Path};

use crate::{
    coverage::{make_coverage_adapter, CoverageAdapter, CoverageFormat},
    domain::{
        Complexity, CoreError, Coverage, Diagnostic, DiagnosticCategory, GroupName, GroupRoot,
        Report, ReportGroup, ReportRow, SourceFile,
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

/// Validate the source-analysis boundary without requiring coverage evidence.
/// The CLI uses this during package-group preflight so syntax failures in a
/// later group cannot trigger generation side effects for an earlier group.
pub fn validate_sources(sources: &[SourceFile]) -> Result<(), CoreError> {
    for source_file in sources {
        source::analyze_source(&source_file.path, &source_file.source)?;
    }
    Ok(())
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
    let coverage = make_coverage_adapter(CoverageFormat::Istanbul, coverage_json, root)?;
    analyze_with_adapter_and_policy(
        sources,
        coverage.as_ref(),
        &ThresholdPolicy::new(threshold),
        allow_unknown,
    )
}

/// Analyze source files with an Istanbul artifact and exact path thresholds.
pub fn analyze_with_policy(
    sources: &[SourceFile],
    coverage_json: &str,
    root: &Path,
    policy: &ThresholdPolicy,
    allow_unknown: bool,
) -> Result<Report, CoreError> {
    let coverage = make_coverage_adapter(CoverageFormat::Istanbul, coverage_json, root)?;
    analyze_with_adapter_and_policy(sources, coverage.as_ref(), policy, allow_unknown)
}

/// Analyze source files with an already constructed coverage adapter.
pub fn analyze_with_adapter(
    sources: &[SourceFile],
    coverage_adapter: &dyn CoverageAdapter,
    threshold: u32,
    allow_unknown: bool,
) -> Result<Report, CoreError> {
    analyze_with_adapter_and_policy(
        sources,
        coverage_adapter,
        &ThresholdPolicy::new(threshold),
        allow_unknown,
    )
}

/// Common application pipeline for every coverage adapter and gate policy.
pub fn analyze_with_adapter_and_policy(
    sources: &[SourceFile],
    coverage_adapter: &dyn CoverageAdapter,
    policy: &ThresholdPolicy,
    allow_unknown: bool,
) -> Result<Report, CoreError> {
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
                diagnostics.sort_by(|left, right| {
                    left.category
                        .cmp(&right.category)
                        .then_with(|| left.message.cmp(&right.message))
                });
                return Err(CoreError::MissingEvidence {
                    path: unit.path.to_string(),
                    reason: reason.clone(),
                    diagnostics: diagnostics.clone(),
                });
            }
        }
        let crap = coverage.fraction().map(|fraction| {
            crap_score(unit.complexity, fraction).expect("validated coverage fraction")
        });
        rows.push(ReportRow {
            id: unit.id.clone(),
            group: None,
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
        threshold: Some(policy.global()),
        rows,
        diagnostics,
        groups: Vec::new(),
    })
}

/// A completed package analysis awaiting aggregate assembly.
///
/// The CLI constructs one of these only after source selection, coverage
/// acquisition, parsing, attribution, and scoring have all succeeded. This
/// makes aggregate rendering an all-or-nothing operation: a later package
/// failure can never expose an earlier package's report.
#[derive(Clone, Debug)]
pub struct PackageReport {
    pub name: GroupName,
    /// Repository-root-relative package root ("." for the root itself).
    pub root: GroupRoot,
    pub policy: ThresholdPolicy,
    pub report_only: bool,
    pub report: Report,
}

/// Assemble completed package reports into the deterministic version-2
/// aggregate document. Rows retain their local source ranges and coverage,
/// while path and id identities are qualified with the package root/name so
/// same-named functions in different packages cannot collide.
pub fn aggregate_reports(mut packages: Vec<PackageReport>) -> Result<Report, CoreError> {
    packages.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.root.cmp(&right.root))
    });

    for pair in packages.windows(2) {
        if pair[0].name == pair[1].name {
            return Err(CoreError::InvalidAggregateIdentity(format!(
                "duplicate package group name '{}'",
                pair[0].name
            )));
        }
    }

    let mut rows = Vec::new();
    let mut diagnostics = Vec::new();
    let mut groups = Vec::with_capacity(packages.len());
    for package in packages {
        let PackageReport {
            name,
            root,
            policy,
            report_only,
            mut report,
        } = package;
        let local_root = if root.as_str() == "." {
            String::new()
        } else {
            root.as_str().to_string()
        };
        for mut row in report.rows.drain(..) {
            let local_path = row.path.clone();
            let qualified_path = if local_root.is_empty() {
                local_path.to_string()
            } else {
                format!("{local_root}/{}", local_path.as_str())
            };
            // Both the group root and local row path are validated values,
            // but the public aggregate boundary still validates their
            // composition before changing either identity. Never leave a
            // local path beside a repository-qualified id on failure.
            row.path =
                crate::domain::ProjectRelativePath::new(&qualified_path).map_err(|error| {
                    CoreError::InvalidAggregateIdentity(format!(
                        "group '{}' composed invalid row path '{}': {error}",
                        name, qualified_path
                    ))
                })?;
            let suffix = row
                .id
                .strip_prefix(local_path.as_str())
                .unwrap_or(row.id.as_str());
            row.id = format!("{name}::{qualified_path}{suffix}");
            row.group = Some(name.to_string());
            rows.push(row);
        }
        diagnostics.extend(
            report
                .diagnostics
                .drain(..)
                .map(|diagnostic| diagnostic.with_group(name.to_string())),
        );
        groups.push(ReportGroup {
            name,
            root,
            threshold: policy.global(),
            report_only,
            threshold_overrides: policy.path_overrides().clone(),
        });
    }

    rows.sort_by(compare_rows);
    diagnostics.sort_by(compare_diagnostics);
    Ok(Report {
        version: crate::domain::AGGREGATE_REPORT_VERSION,
        threshold: None,
        rows,
        diagnostics,
        groups,
    })
}

fn compare_diagnostics(left: &Diagnostic, right: &Diagnostic) -> Ordering {
    left.group
        .cmp(&right.group)
        .then_with(|| left.category.cmp(&right.category))
        .then_with(|| left.message.cmp(&right.message))
}

/// Analyze an artifact after selecting its coverage format.
pub fn analyze_with_root_and_format(
    sources: &[SourceFile],
    coverage_input: &str,
    root: &Path,
    format: CoverageFormat,
    threshold: u32,
    allow_unknown: bool,
) -> Result<Report, CoreError> {
    let adapter = make_coverage_adapter(format, coverage_input, root)?;
    analyze_with_adapter_and_policy(
        sources,
        adapter.as_ref(),
        &ThresholdPolicy::new(threshold),
        allow_unknown,
    )
}

fn compare_rows(left: &ReportRow, right: &ReportRow) -> Ordering {
    let group_order = || left.group.cmp(&right.group);
    match (left.crap, right.crap) {
        (Some(left_score), Some(right_score)) => right_score
            .partial_cmp(&left_score)
            .unwrap_or(Ordering::Equal)
            .then_with(group_order)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.range.start.offset.cmp(&right.range.start.offset))
            .then_with(|| left.range.end.offset.cmp(&right.range.end.offset))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.id.cmp(&right.id)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => left
            .group
            .cmp(&right.group)
            .then_with(|| left.path.cmp(&right.path))
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
            group: None,
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

    #[test]
    fn aggregate_reports_qualify_repository_identity_and_sort_groups() {
        let first = crate::domain::Report {
            version: crate::domain::REPORT_VERSION,
            threshold: Some(8),
            rows: vec![row("src/file.ts", Some(2.0))],
            diagnostics: vec![Diagnostic::new(
                DiagnosticCategory::CoverageAttribution,
                "stale coverage",
            )],
            groups: Vec::new(),
        };
        let second = crate::domain::Report {
            version: crate::domain::REPORT_VERSION,
            threshold: Some(8),
            rows: vec![row("src/file.ts", Some(2.0))],
            diagnostics: Vec::new(),
            groups: Vec::new(),
        };
        let report = aggregate_reports(vec![
            PackageReport {
                name: GroupName::new("z").unwrap(),
                root: GroupRoot::new("packages/z").unwrap(),
                policy: ThresholdPolicy::new(8),
                report_only: false,
                report: first,
            },
            PackageReport {
                name: GroupName::new("a").unwrap(),
                root: GroupRoot::new("packages/a").unwrap(),
                policy: ThresholdPolicy::new(8),
                report_only: false,
                report: second,
            },
        ])
        .unwrap();
        assert_eq!(report.version, crate::domain::AGGREGATE_REPORT_VERSION);
        assert_eq!(report.threshold, None);
        assert_eq!(
            report
                .groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert_eq!(report.rows[0].group.as_deref(), Some("a"));
        assert_eq!(report.rows[0].path.as_str(), "packages/a/src/file.ts");
        assert_eq!(report.rows[0].id, "a::packages/a/src/file.ts::f");
        assert_eq!(report.diagnostics[0].group.as_deref(), Some("z"));
    }

    #[test]
    fn aggregate_reports_rejects_duplicate_validated_group_names() {
        let report = crate::domain::Report {
            version: crate::domain::REPORT_VERSION,
            threshold: Some(8),
            rows: Vec::new(),
            diagnostics: Vec::new(),
            groups: Vec::new(),
        };
        let package = |root: &str| PackageReport {
            name: GroupName::new("core").unwrap(),
            root: GroupRoot::new(root).unwrap(),
            policy: ThresholdPolicy::new(8),
            report_only: false,
            report: report.clone(),
        };
        assert!(matches!(
            aggregate_reports(vec![package("packages/core-a"), package("packages/core-b")]),
            Err(CoreError::InvalidAggregateIdentity(_))
        ));
    }
}
