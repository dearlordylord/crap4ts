//! Application pipeline and quality-gate policy.

use std::{cmp::Ordering, path::Path};

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
    coverage_adapter.validate_attribution(&units, sources)?;

    let mut rows = Vec::with_capacity(units.len());
    for unit in &units {
        let source = sources
            .iter()
            .find(|source| source.path == unit.path)
            .map_or("", |source| source.source.as_str());
        let measured = coverage_adapter.coverage_for(&unit.path, unit, source, &units)?;
        let coverage = measured.unwrap_or_else(|| {
            Coverage::unknown("no matching Istanbul function or statement evidence")
        });
        if let Coverage::Unknown { reason } = &coverage {
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
        version: crate::domain::REPORT_VERSION,
        threshold,
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
}
