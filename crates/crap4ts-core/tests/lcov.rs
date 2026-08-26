use std::{path::Path, str::FromStr};

use crap4ts_core::{
    analyze_with_adapter, analyze_with_root_and_format, make_coverage_adapter, CoreError, Coverage,
    CoverageFormat, DiagnosticCategory, ProjectRelativePath, SourceFile,
};

const LCOV: &str = include_str!("fixtures/lcov/coverage.info");
const COVERAGE_SOURCE: &str = include_str!("fixtures/lcov/coverage.ts");
const DUPLICATE_A: &str = include_str!("fixtures/lcov/duplicate-a.ts");
const DUPLICATE_B: &str = include_str!("fixtures/lcov/duplicate-b.ts");
const AMBIGUOUS_SOURCE: &str = include_str!("fixtures/lcov/ambiguous.ts");

fn source(path: &str, content: &str) -> SourceFile {
    SourceFile {
        path: ProjectRelativePath::new(path).expect("fixture path"),
        source: content.to_string(),
    }
}

#[test]
fn lcov_normalizes_full_partial_zero_missing_nested_and_duplicate_names() {
    let sources = [
        source("coverage.ts", COVERAGE_SOURCE),
        source("duplicate-a.ts", DUPLICATE_A),
        source("duplicate-b.ts", DUPLICATE_B),
    ];
    let adapter = make_coverage_adapter(CoverageFormat::Lcov, LCOV, Path::new("."))
        .expect("parse LCOV fixture");
    let report = analyze_with_adapter(&sources, adapter.as_ref(), 8, true).expect("analyze");

    let row = |path: &str, name: &str| {
        report
            .rows
            .iter()
            .find(|row| row.path.as_str() == path && row.name == name)
            .unwrap_or_else(|| panic!("missing row {path}::{name}"))
    };
    assert_eq!(
        row("coverage.ts", "full").coverage,
        Coverage::measured(1, 1).unwrap()
    );
    assert_eq!(
        row("coverage.ts", "partial").coverage,
        Coverage::measured(2, 3).unwrap()
    );
    assert_eq!(
        row("coverage.ts", "zero").coverage,
        Coverage::measured(0, 1).unwrap()
    );
    assert!(matches!(
        row("coverage.ts", "missing").coverage,
        Coverage::Unknown { .. }
    ));
    assert_eq!(
        row("coverage.ts", "outer").coverage,
        Coverage::measured(1, 1).unwrap()
    );
    assert_eq!(
        row("coverage.ts", "child").coverage,
        Coverage::measured(1, 1).unwrap()
    );
    assert_eq!(
        row("duplicate-a.ts", "same").coverage,
        Coverage::measured(1, 1).unwrap()
    );
    assert_eq!(
        row("duplicate-b.ts", "same").coverage,
        Coverage::measured(0, 1).unwrap()
    );
}

#[test]
fn lcov_same_line_functions_are_unknown_with_structured_diagnostic() {
    let sources = [source("ambiguous.ts", AMBIGUOUS_SOURCE)];
    let ambiguous_lcov = "TN:\nSF:ambiguous.ts\nFN:1,first\nFN:1,second\nFNDA:1,first\nFNDA:1,second\nFNF:2\nFNH:2\nDA:1,1\nLF:1\nLH:1\nend_of_record\n";
    let report = analyze_with_root_and_format(
        &sources,
        ambiguous_lcov,
        Path::new("."),
        CoverageFormat::Lcov,
        8,
        true,
    )
    .expect("analyze ambiguous LCOV");
    assert!(report
        .rows
        .iter()
        .all(|row| matches!(row.coverage, Coverage::Unknown { .. })));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.category == DiagnosticCategory::CoverageAttribution
            && diagnostic.message.contains("ambiguous LCOV")
            && diagnostic.message.contains("line-only")
    }));
}

#[test]
fn lcov_missing_evidence_never_becomes_zero_and_strict_mode_rejects_it() {
    let sources = [source("coverage.ts", COVERAGE_SOURCE)];
    let adapter = make_coverage_adapter(
        CoverageFormat::Lcov,
        "TN:\nSF:coverage.ts\nFNF:0\nFNH:0\nLF:0\nLH:0\nend_of_record\n",
        Path::new("."),
    )
    .expect("parse missing LCOV");
    let report = analyze_with_adapter(&sources, adapter.as_ref(), 8, true).expect("report-only");
    assert!(report
        .rows
        .iter()
        .all(|row| matches!(row.coverage, Coverage::Unknown { .. })));
    assert!(report.rows.iter().all(|row| row.crap.is_none()));

    let strict = analyze_with_adapter(&sources, adapter.as_ref(), 8, false).unwrap_err();
    assert!(matches!(strict, CoreError::MissingEvidence { .. }));
}

#[test]
fn coverage_format_and_factory_are_shared_by_cli_and_config_callers() {
    assert_eq!(
        CoverageFormat::from_str("lcov").unwrap(),
        CoverageFormat::Lcov
    );
    assert_eq!(
        CoverageFormat::from_str("istanbul-json").unwrap(),
        CoverageFormat::Istanbul
    );
    assert_eq!(CoverageFormat::Lcov.to_string(), "lcov");
    assert!(CoverageFormat::from_str("text").is_err());

    let sources = [source("coverage.ts", COVERAGE_SOURCE)];
    let report = analyze_with_root_and_format(
        &sources,
        LCOV,
        Path::new("."),
        CoverageFormat::Lcov,
        8,
        true,
    )
    .expect("factory-selected analysis");
    assert_eq!(
        report
            .rows
            .iter()
            .find(|row| row.name == "full")
            .unwrap()
            .coverage,
        Coverage::measured(1, 1).unwrap()
    );
}

#[test]
fn lcov_rejects_duplicate_file_records_and_out_of_range_lines() {
    let duplicate =
        "TN:\nSF:coverage.ts\nDA:2,1\nend_of_record\nSF:./coverage.ts\nDA:2,1\nend_of_record\n";
    let error = make_coverage_adapter(CoverageFormat::Lcov, duplicate, Path::new("."))
        .err()
        .expect("duplicate LCOV file must fail");
    assert!(matches!(error, CoreError::AmbiguousCoverageFile(_)));

    let source = [source("coverage.ts", "function f() { return 1; }\n")];
    let out_of_range = "TN:\nSF:coverage.ts\nDA:3,1\nend_of_record\n";
    let adapter = make_coverage_adapter(CoverageFormat::Lcov, out_of_range, Path::new("."))
        .expect("parse out-of-range artifact");
    let error = analyze_with_adapter(&source, adapter.as_ref(), 8, true).unwrap_err();
    assert!(matches!(error, CoreError::CoverageParsing(_)));
}

#[test]
fn lcov_counts_da_lines_on_multiline_function_signatures() {
    let source = [source("multiline.ts", "function f()\n{\n  return 1;\n}\n")];
    let artifact = "TN:\nSF:multiline.ts\nFN:1,f\nFNDA:1,f\nFNF:1\nFNH:1\nDA:1,1\nDA:3,0\nLF:2\nLH:1\nend_of_record\n";
    let report = analyze_with_root_and_format(
        &source,
        artifact,
        Path::new("."),
        CoverageFormat::Lcov,
        8,
        true,
    )
    .expect("analyze multiline function");
    assert_eq!(report.rows.len(), 1);
    assert_eq!(report.rows[0].coverage, Coverage::measured(1, 2).unwrap());
}

#[test]
fn lcov_rejects_a_line_shared_with_top_level_or_closing_code() {
    let sources = [
        source(
            "prefix.ts",
            "const before = 0; function f() { return 1; }\n",
        ),
        source("suffix.ts", "function f() { return 1; } const after = 0;\n"),
        source("arrow.ts", "const f = () => 1;\n"),
    ];
    let artifact = "TN:\nSF:prefix.ts\nFN:1,f\nFNDA:1,f\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:suffix.ts\nFN:1,f\nFNDA:1,f\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:arrow.ts\nFN:1,f\nFNDA:1,f\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\n";
    let report = analyze_with_root_and_format(
        &sources,
        artifact,
        Path::new("."),
        CoverageFormat::Lcov,
        8,
        true,
    )
    .expect("analyze line-sharing LCOV");
    assert!(report
        .rows
        .iter()
        .filter(|row| row.path.as_str() != "arrow.ts")
        .all(|row| matches!(row.coverage, Coverage::Unknown { .. })));
    assert_eq!(
        report
            .rows
            .iter()
            .find(|row| row.path.as_str() == "arrow.ts")
            .unwrap()
            .coverage,
        Coverage::measured(1, 1).unwrap()
    );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.category == DiagnosticCategory::CoverageAttribution)
            .count(),
        2
    );
}

#[test]
fn lcov_accepts_standalone_method_and_accessor_declaration_envelopes() {
    let sources = [
        source("method.ts", "class C { method() { return 1; } }\n"),
        source("getter.ts", "class C { get value() { return 1; } }\n"),
    ];
    let artifact = "TN:\nSF:method.ts\nFN:1,method\nFNDA:1,method\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\nSF:getter.ts\nFN:1,value\nFNDA:1,value\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\n";
    let report = analyze_with_root_and_format(
        &sources,
        artifact,
        Path::new("."),
        CoverageFormat::Lcov,
        8,
        true,
    )
    .expect("analyze standalone declarations");
    assert!(report
        .rows
        .iter()
        .all(|row| row.coverage == Coverage::measured(1, 1).unwrap()));
}

#[test]
fn lcov_does_not_measure_arrow_inside_same_line_control_flow() {
    let sources = [source(
        "control-arrow.ts",
        "if (condition) { const f = () => 1; }\n",
    )];
    let artifact = "TN:\nSF:control-arrow.ts\nFN:1,f\nFNDA:1,f\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\n";
    let report = analyze_with_root_and_format(
        &sources,
        artifact,
        Path::new("."),
        CoverageFormat::Lcov,
        8,
        true,
    )
    .expect("analyze control-flow arrow");
    assert!(matches!(report.rows[0].coverage, Coverage::Unknown { .. }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.category == DiagnosticCategory::CoverageAttribution
            && diagnostic.message.contains("ambiguous LCOV")
    }));
}

#[test]
fn lcov_rejects_unknown_records_malformed_branches_and_inconsistent_summaries() {
    for artifact in [
        "TN:\nSF:one.ts\nNOPE:value\nend_of_record\n",
        "TN:\nSF:one.ts\nBRDA:1,0,0\nend_of_record\n",
        "TN:\nSF:one.ts\nFN:1,f\nFNDA:1,f\nFNF:2\nFNH:1\nend_of_record\n",
    ] {
        let error = make_coverage_adapter(CoverageFormat::Lcov, artifact, Path::new("."))
            .err()
            .expect("malformed LCOV must fail");
        assert!(matches!(error, CoreError::CoverageParsing(_)));
    }
}
