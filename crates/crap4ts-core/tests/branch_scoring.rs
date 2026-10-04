use crap4ts_core::{
    analyze_with_adapter, make_coverage_adapter_with_metric, CoreError, Coverage, CoverageBasis,
    CoverageFormat, CoverageMetric, ProjectRelativePath, Report, SourceFile,
};
use serde_json::{json, Value};
use std::path::Path;

fn source(text: &str) -> SourceFile {
    SourceFile {
        path: ProjectRelativePath::new("sample.ts").unwrap(),
        source: text.to_string(),
    }
}

fn analyze(
    source: &SourceFile,
    artifact: &str,
    format: CoverageFormat,
    metric: CoverageMetric,
    unknown: bool,
) -> Result<Report, CoreError> {
    let adapter = make_coverage_adapter_with_metric(format, artifact, Path::new("."), metric)?;
    analyze_with_adapter(std::slice::from_ref(source), adapter.as_ref(), 2, unknown)
}

fn range(range: crap4ts_core::SourceRange) -> Value {
    json!({"start":{"line":range.start.line,"column":range.start.column},"end":{"line":range.end.line,"column":range.end.column}})
}

#[test]
fn istanbul_branches_change_the_gate_and_keep_nested_outcomes_exclusive() {
    let file = source("function outer(v: boolean) {\n  if (v) return 1;\n  const child = (x: boolean) => {\n    if (x) return 2;\n    return 3;\n  };\n  return child(v);\n}\n");
    let units = analyze(
        &file,
        "{}",
        CoverageFormat::Istanbul,
        CoverageMetric::Legacy,
        true,
    )
    .unwrap();
    let mut artifact =
        json!({"sample.ts":{"fnMap":{},"f":{},"statementMap":{},"s":{},"branchMap":{},"b":{}}});
    for (index, row) in units.rows.iter().enumerate() {
        let id = index.to_string();
        let entry = &mut artifact["sample.ts"];
        entry["fnMap"][&id] = json!({"name":row.name,"loc":range(row.body_range)});
        entry["f"][&id] = json!(1);
        let line = if row.name == "outer" { 2 } else { 4 };
        let loc = json!({"start":{"line":line,"column":2},"end":{"line":line,"column":file.source.lines().nth(line - 1).unwrap().len()}});
        entry["branchMap"][&id] = json!({"loc":loc,"locations":[loc,loc]});
        entry["b"][&id] = if row.name == "outer" {
            json!([1, 0])
        } else {
            json!([1, 1])
        };
    }
    let legacy = analyze(
        &file,
        &artifact.to_string(),
        CoverageFormat::Istanbul,
        CoverageMetric::Legacy,
        false,
    )
    .unwrap();
    assert!(legacy.rows.iter().all(|row| row.crap == Some(2.0)));
    let report = analyze(
        &file,
        &artifact.to_string(),
        CoverageFormat::Istanbul,
        CoverageMetric::Branch,
        false,
    )
    .unwrap();
    assert_eq!(report.version, 3);
    let outer = report.rows.iter().find(|row| row.name == "outer").unwrap();
    assert_eq!(
        outer.coverage,
        Coverage::measured(1, 2)
            .unwrap()
            .with_basis(CoverageBasis::Branch)
    );
    assert_eq!(outer.crap, Some(2.5));
    let child = report.rows.iter().find(|row| row.name == "child").unwrap();
    assert_eq!(
        child.coverage,
        Coverage::measured(2, 2)
            .unwrap()
            .with_basis(CoverageBasis::Branch)
    );
}

#[test]
fn lcov_branch_scoring_reports_line_fallback_and_missing_evidence() {
    let file = source("function choose(v: boolean) {\n  return v ? 1 : 2;\n}\nfunction plain() {\n  return 1;\n}\nfunction absent() { return 1; }\n");
    let artifact =
        "SF:sample.ts\nDA:2,1\nDA:5,1\nBRDA:2,0,0,1\nBRDA:2,0,1,-\nBRF:2\nBRH:1\nend_of_record\n";
    let legacy = analyze(
        &file,
        artifact,
        CoverageFormat::Lcov,
        CoverageMetric::Legacy,
        true,
    )
    .unwrap();
    assert_eq!(
        legacy
            .rows
            .iter()
            .find(|row| row.name == "choose")
            .unwrap()
            .crap,
        Some(2.0)
    );
    let report = analyze(
        &file,
        artifact,
        CoverageFormat::Lcov,
        CoverageMetric::Branch,
        true,
    )
    .unwrap();
    assert_eq!(
        report
            .rows
            .iter()
            .find(|row| row.name == "choose")
            .unwrap()
            .crap,
        Some(2.5)
    );
    assert_eq!(
        report
            .rows
            .iter()
            .find(|row| row.name == "plain")
            .unwrap()
            .coverage,
        Coverage::measured(1, 1)
            .unwrap()
            .with_basis(CoverageBasis::Line)
    );
    assert!(matches!(
        report
            .rows
            .iter()
            .find(|row| row.name == "absent")
            .unwrap()
            .coverage,
        Coverage::Unknown { .. }
    ));
    assert!(matches!(
        analyze(
            &file,
            artifact,
            CoverageFormat::Lcov,
            CoverageMetric::Branch,
            false
        ),
        Err(CoreError::MissingEvidence { .. })
    ));
}

#[test]
fn lcov_branches_remain_unknown_on_shared_lines_and_validate_records() {
    let file = source("const a = () => true ? 1 : 0; const b = () => 2;\n");
    let artifact = "SF:sample.ts\nBRDA:1,0,0,1\nBRDA:1,0,1,0\nend_of_record\n";
    let report = analyze(
        &file,
        artifact,
        CoverageFormat::Lcov,
        CoverageMetric::Branch,
        true,
    )
    .unwrap();
    assert!(report
        .rows
        .iter()
        .all(|row| matches!(row.coverage, Coverage::Unknown { .. })));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("ambiguous LCOV")));
    for invalid in [
        "SF:sample.ts\nBRDA:1,0,0,1\nBRDA:1,0,0,1\nend_of_record\n",
        "SF:sample.ts\nBRDA:1,0,0,1\nBRF:2\nend_of_record\n",
        "SF:sample.ts\nBRDA:99,0,0,1\nend_of_record\n",
    ] {
        assert!(matches!(
            analyze(
                &file,
                invalid,
                CoverageFormat::Lcov,
                CoverageMetric::Branch,
                true
            ),
            Err(CoreError::CoverageParsing(_))
        ));
    }
}

#[test]
fn istanbul_branch_metadata_is_validated_and_unmatched_children_block_fallback() {
    let file = source("function outer() {\n  const child = () => {\n    return true ? 1 : 0;\n  };\n  return child();\n}\n");
    let units = analyze(
        &file,
        "{}",
        CoverageFormat::Istanbul,
        CoverageMetric::Legacy,
        true,
    )
    .unwrap();
    let outer = units.rows.iter().find(|row| row.name == "outer").unwrap();
    let loc = json!({"start":{"line":3,"column":4},"end":{"line":3,"column":23}});
    let mut artifact = json!({"sample.ts":{
        "fnMap":{"0":{"name":"outer","loc":range(outer.body_range)}},"f":{"0":1},
        "branchMap":{"0":{"loc":loc,"locations":[loc,loc]}},"b":{"0":[1,0]}
    }});
    let report = analyze(
        &file,
        &artifact.to_string(),
        CoverageFormat::Istanbul,
        CoverageMetric::Branch,
        true,
    )
    .unwrap();
    assert!(report
        .rows
        .iter()
        .all(|row| matches!(row.coverage, Coverage::Unknown { .. })));
    artifact["sample.ts"]["b"]["0"] = json!([1]);
    assert!(matches!(
        analyze(
            &file,
            &artifact.to_string(),
            CoverageFormat::Istanbul,
            CoverageMetric::Branch,
            true
        ),
        Err(CoreError::CoverageParsing(_))
    ));
}

#[test]
fn istanbul_fallback_basis_and_route_schema_are_explicit() {
    let file = source("import express from 'express';\nconst app = express();\napp.get('/users', function handler() { return 1; });\n");
    let units = analyze(
        &file,
        "{}",
        CoverageFormat::Istanbul,
        CoverageMetric::Legacy,
        true,
    )
    .unwrap();
    assert_eq!(units.version, 3);
    let row = &units.rows[0];
    assert_eq!(row.name, "handler");
    assert_eq!(row.label.as_deref(), Some("GET /users"));
    let mut artifact = json!({"sample.ts":{"fnMap":{"0":{"name":"handler","loc":range(row.body_range)}},"f":{"0":1}}});
    let report = analyze(
        &file,
        &artifact.to_string(),
        CoverageFormat::Istanbul,
        CoverageMetric::Branch,
        false,
    )
    .unwrap();
    assert_eq!(
        report.rows[0].coverage,
        Coverage::measured(1, 1)
            .unwrap()
            .with_basis(CoverageBasis::Function)
    );
    artifact["sample.ts"]["statementMap"] = json!({"0":range(row.body_range)});
    artifact["sample.ts"]["s"] = json!({"0":0});
    let report = analyze(
        &file,
        &artifact.to_string(),
        CoverageFormat::Istanbul,
        CoverageMetric::Branch,
        false,
    )
    .unwrap();
    assert_eq!(
        report.rows[0].coverage,
        Coverage::measured(0, 1)
            .unwrap()
            .with_basis(CoverageBasis::Statement)
    );
    assert!(crap4ts_core::render_text(&report).contains("GET /users"));
}

#[test]
fn istanbul_parameter_branches_and_implicit_else_locations_are_measured() {
    let file = source("function choose(value = true) {\n  if (value) return 1;\n  return 0;\n}\n");
    let units = analyze(
        &file,
        "{}",
        CoverageFormat::Istanbul,
        CoverageMetric::Legacy,
        true,
    )
    .unwrap();
    let loc = json!({"start":{"line":2,"column":2},"end":{"line":2,"column":22}});
    let parameter = json!({"start":{"line":1,"column":16},"end":{"line":1,"column":28}});
    let artifact = json!({"sample.ts":{
        "fnMap":{"0":{"name":"choose","loc":range(units.rows[0].body_range)}},"f":{"0":1},
        "branchMap":{"0":{"loc":loc,"locations":[loc,{"start":{},"end":{}}]},"1":{"loc":parameter,"locations":[parameter]}},"b":{"0":[1,0],"1":[0]}
    }});
    let report = analyze(
        &file,
        &artifact.to_string(),
        CoverageFormat::Istanbul,
        CoverageMetric::Branch,
        false,
    )
    .unwrap();
    assert_eq!(
        report.rows[0].coverage,
        Coverage::measured(1, 3)
            .unwrap()
            .with_basis(CoverageBasis::Branch)
    );
    let mut malformed = artifact;
    malformed["sample.ts"]["branchMap"]["0"]["locations"][1] =
        json!({"start":{},"end":{"line":2,"column":22}});
    assert!(matches!(
        analyze(
            &file,
            &malformed.to_string(),
            CoverageFormat::Istanbul,
            CoverageMetric::Branch,
            true
        ),
        Err(CoreError::CoverageParsing(_))
    ));
}

#[test]
fn lcov_nested_branch_outcomes_have_exclusive_owners_without_line_records() {
    let file = source("function outer(v: boolean) {\n  if (v) return 1;\n  const child = (x: boolean) => {\n    if (x) return 2;\n    return 3;\n  };\n  return child(v);\n}\n");
    let artifact =
        "SF:sample.ts\nBRDA:2,0,0,1\nBRDA:2,0,1,0\nBRDA:4,1,0,1\nBRDA:4,1,1,1\nend_of_record\n";
    let report = analyze(
        &file,
        artifact,
        CoverageFormat::Lcov,
        CoverageMetric::Branch,
        false,
    )
    .unwrap();
    let outer = report.rows.iter().find(|row| row.name == "outer").unwrap();
    let child = report.rows.iter().find(|row| row.name == "child").unwrap();
    assert_eq!(
        outer.coverage,
        Coverage::measured(1, 2)
            .unwrap()
            .with_basis(CoverageBasis::Branch)
    );
    assert_eq!(
        child.coverage,
        Coverage::measured(2, 2)
            .unwrap()
            .with_basis(CoverageBasis::Branch)
    );
}
