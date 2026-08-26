use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use serde_json::{json, Value};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("crap4ts-cli-{}-{id}", std::process::id()));
        fs::create_dir_all(root.join("src")).expect("create fixture");
        let source = "function greet(name: string) { return name; }\n";
        fs::write(root.join("src/fixture.ts"), source).expect("write source");
        let absolute = root.join("src/fixture.ts");
        let coverage = json!({
            absolute.to_string_lossy(): {
                "path": absolute.to_string_lossy(),
                "statementMap": {
                    "0": {"start": {"line": 1, "column": 31}, "end": {"line": 1, "column": 43}}
                },
                "fnMap": {
                    "0": {
                        "name": "greet",
                        "decl": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 45}},
                        "loc": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 45}}
                    }
                },
                "s": {"0": 1},
                "f": {"0": 1}
            }
        });
        fs::write(
            root.join("coverage-final.json"),
            serde_json::to_vec_pretty(&coverage).expect("encode coverage"),
        )
        .expect("write coverage");
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct MixedFixture {
    root: PathBuf,
}

impl MixedFixture {
    fn new() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("crap4ts-cli-mixed-{}-{id}", std::process::id()));
        fs::create_dir_all(root.join("packages/istanbul/src")).expect("create Istanbul package");
        fs::create_dir_all(root.join("packages/lcov/src")).expect("create LCOV package");
        let istanbul_source = root.join("packages/istanbul/src/fixture.ts");
        let lcov_source = root.join("packages/lcov/src/fixture.ts");
        fs::write(
            &istanbul_source,
            "function greet(name: string) { return name; }\n",
        )
        .expect("write Istanbul source");
        fs::write(
            &lcov_source,
            "function greet(name: string) { return name; }\n",
        )
        .expect("write LCOV source");

        let coverage = json!({
            istanbul_source.to_string_lossy(): {
                "path": istanbul_source.to_string_lossy(),
                "statementMap": {
                    "0": {"start": {"line": 1, "column": 31}, "end": {"line": 1, "column": 43}}
                },
                "fnMap": {
                    "0": {
                        "name": "greet",
                        "decl": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 45}},
                        "loc": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 45}}
                    }
                },
                "s": {"0": 1},
                "f": {"0": 1}
            }
        });
        fs::write(
            root.join("packages/istanbul/coverage-final.json"),
            serde_json::to_vec(&coverage).expect("encode Istanbul coverage"),
        )
        .expect("write Istanbul coverage");
        fs::write(
            root.join("packages/lcov/lcov.info"),
            "TN:\nSF:src/fixture.ts\nFN:1,greet\nFNDA:1,greet\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
        )
        .expect("write LCOV coverage");
        Self { root }
    }

    fn config(&self, groups: Value) {
        fs::write(
            self.root.join("crap4ts.json"),
            serde_json::to_vec(&json!({"format": "json", "groups": groups}))
                .expect("encode mixed config"),
        )
        .expect("write mixed config");
    }
}

impl Drop for MixedFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn binary() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_BIN_EXE_crap4ts").expect("cargo binary path"))
}

#[test]
fn help_and_version_are_stable_commands() {
    let help = Command::new(binary())
        .arg("--help")
        .output()
        .expect("run help");
    assert_eq!(help.status.code(), Some(0));
    let help_stdout = String::from_utf8(help.stdout).expect("help output");
    assert!(help_stdout.contains("Usage: crap4ts"));
    assert!(help_stdout.contains("--coverage"));

    let version = Command::new(binary())
        .arg("--version")
        .output()
        .expect("run version");
    assert_eq!(version.status.code(), Some(0));
    assert!(String::from_utf8(version.stdout)
        .expect("version output")
        .starts_with("crap4ts "));
}

fn run(fixture: &Fixture, extra: &[&str]) -> std::process::Output {
    run_with_source(fixture, extra, &["src/fixture.ts"])
}

fn run_with_source(
    fixture: &Fixture,
    extra: &[&str],
    source_paths: &[&str],
) -> std::process::Output {
    let mut command = Command::new(binary());
    command
        .current_dir(&fixture.root)
        .args(["--coverage", "coverage-final.json"])
        .args(source_paths)
        .args(extra);
    command.output().expect("run crap4ts")
}

#[test]
fn black_box_json_report_has_identity_score_and_clean_streams() {
    let fixture = Fixture::new();
    let output = run(&fixture, &["--format", "json", "--threshold", "1"]);
    let repeat = run(&fixture, &["--format", "json", "--threshold", "1"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(repeat.status.code(), Some(0));
    assert_eq!(output.stdout, repeat.stdout);
    assert_eq!(output.stderr, repeat.stderr);
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert_eq!(report["version"], 1);
    assert_eq!(report["rows"][0]["path"], "src/fixture.ts");
    assert_eq!(report["rows"][0]["name"], "greet");
    assert_eq!(report["rows"][0]["complexity"], 1);
    assert_eq!(report["rows"][0]["coverage"]["status"], "measured");
    assert_eq!(report["rows"][0]["coverage"]["fraction"], 1.0);
    assert_eq!(report["rows"][0]["crap"], 1.0);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("timestamp"));
}

#[test]
fn black_box_text_and_threshold_statuses_are_stable() {
    let fixture = Fixture::new();
    let text = run(&fixture, &["--threshold", "1"]);
    assert_eq!(text.status.code(), Some(0));
    let text_stdout = String::from_utf8(text.stdout).expect("text output");
    assert!(text_stdout.contains("greet"));
    assert!(text_stdout.contains("complexity=1"));
    assert!(text_stdout.contains("coverage=100.00%"));
    assert!(text_stdout.contains("crap=1.000000"));

    let breach = run(&fixture, &["--format", "json", "--threshold", "0"]);
    assert_eq!(breach.status.code(), Some(2));
    assert!(serde_json::from_slice::<Value>(&breach.stdout).is_ok());
    assert!(String::from_utf8_lossy(&breach.stderr).contains("quality gate breached"));
}

#[test]
fn lcov_format_selection_uses_the_same_report_contract() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("lcov.info"),
        "TN:\nSF:src/fixture.ts\nFN:1,greet\nFNDA:1,greet\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    )
    .expect("write LCOV fixture");
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .args([
            "--coverage",
            "lcov.info",
            "--coverage-format",
            "lcov",
            "src/fixture.ts",
            "--format",
            "json",
        ])
        .output()
        .expect("run LCOV analysis");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert_eq!(report["rows"][0]["coverage"]["status"], "measured");
    assert_eq!(report["rows"][0]["coverage"]["covered"], 1);
    assert_eq!(report["rows"][0]["coverage"]["total"], 1);
}

#[test]
fn config_selects_lcov_and_explicit_coverage_format_overrides_it() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("coverage.info"),
        "TN:\nSF:src/fixture.ts\nFN:1,greet\nFNDA:1,greet\nFNF:1\nFNH:1\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    )
    .expect("write LCOV fixture");
    fs::write(
        fixture.root.join("crap4ts.json"),
        serde_json::to_vec(&json!({
            "sources": ["src/fixture.ts"],
            "coverage": {"path": "coverage.info", "format": "lcov"},
            "format": "json"
        }))
        .unwrap(),
    )
    .unwrap();

    let configured = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run LCOV config");
    assert_eq!(configured.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&configured.stdout).expect("JSON report");
    assert_eq!(report["rows"][0]["coverage"]["status"], "measured");

    let overridden = Command::new(binary())
        .current_dir(&fixture.root)
        .args(["--coverage-format", "istanbul"])
        .output()
        .expect("run explicit format override");
    assert_eq!(overridden.status.code(), Some(1));
    assert!(overridden.stdout.is_empty());
    assert!(String::from_utf8_lossy(&overridden.stderr).contains("coverage parsing failed"));
}

#[test]
fn strict_lcov_failures_retain_attribution_diagnostics_in_json_stderr() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("src/fixture.ts"),
        "const first = () => 1; const second = () => 2;\n",
    )
    .expect("replace source with ambiguous fixture");
    fs::write(
        fixture.root.join("lcov.info"),
        "TN:\nSF:src/fixture.ts\nFN:1,first\nFN:1,second\nFNDA:1,first\nFNDA:1,second\nFNF:2\nFNH:2\nDA:1,1\nLF:1\nLH:1\nend_of_record\n",
    )
    .expect("write ambiguous LCOV fixture");
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .args([
            "--coverage",
            "lcov.info",
            "--coverage-format",
            "lcov",
            "src/fixture.ts",
            "--format",
            "json",
        ])
        .output()
        .expect("run strict LCOV analysis");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostics: Value = serde_json::from_slice(&output.stderr).expect("structured stderr");
    let categories = diagnostics["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .filter_map(|diagnostic| diagnostic["category"].as_str())
        .collect::<Vec<_>>();
    assert!(categories.contains(&"coverage_attribution"));
    assert!(categories.contains(&"missing_evidence"));
}

#[test]
fn source_identity_is_canonical_for_redundant_path_segments() {
    let fixture = Fixture::new();
    let output = run_with_source(&fixture, &["--format", "json"], &["src/../src/fixture.ts"]);
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert_eq!(report["rows"][0]["path"], "src/fixture.ts");
}

#[test]
fn source_identity_accepts_windows_separators() {
    let fixture = Fixture::new();
    let output = run_with_source(&fixture, &["--format", "json"], &[r"src\fixture.ts"]);
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert_eq!(report["rows"][0]["path"], "src/fixture.ts");
}

#[cfg(windows)]
#[test]
fn source_identity_accepts_posix_separators_on_windows() {
    let fixture = Fixture::new();
    let output = run_with_source(&fixture, &["--format", "json"], &["src/fixture.ts"]);
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert_eq!(report["rows"][0]["path"], "src/fixture.ts");
}

#[test]
fn absolute_source_inside_project_root_is_accepted() {
    let fixture = Fixture::new();
    let source = fixture.root.join("src/fixture.ts");
    let output = run_with_source(
        &fixture,
        &["--format", "json"],
        &[source.to_str().expect("UTF-8 fixture path")],
    );
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    assert_eq!(report["rows"][0]["path"], "src/fixture.ts");
}

#[test]
fn malformed_coverage_is_an_analysis_error() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("coverage-final.json"), b"{not json").expect("replace coverage");
    let output = run(&fixture, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("coverage parsing failed"));
}

#[test]
fn coverage_paths_must_be_exact_project_identities() {
    let fixture = Fixture::new();
    let coverage_path = fixture.root.join("coverage-final.json");
    let source_path = fixture.root.join("src/fixture.ts");
    let mut coverage: Value = serde_json::from_slice(&fs::read(&coverage_path).unwrap()).unwrap();
    let source_key = source_path.to_string_lossy().to_string();
    let mut entry = coverage
        .as_object_mut()
        .unwrap()
        .remove(&source_key)
        .unwrap();
    entry["path"] = Value::String("/definitely-outside-crap4ts/src/fixture.ts".to_string());
    coverage.as_object_mut().unwrap().insert(
        "/definitely-outside-crap4ts/src/fixture.ts".to_string(),
        entry,
    );
    fs::write(&coverage_path, serde_json::to_vec(&coverage).unwrap()).unwrap();
    let unrelated = run(&fixture, &["--report-only"]);
    assert_eq!(unrelated.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unrelated.stderr).contains("coverage parsing failed"));

    let fixture = Fixture::new();
    let coverage_path = fixture.root.join("coverage-final.json");
    let mut coverage: Value = serde_json::from_slice(&fs::read(&coverage_path).unwrap()).unwrap();
    let source_key = fixture
        .root
        .join("src/fixture.ts")
        .to_string_lossy()
        .to_string();
    let mut entry = coverage
        .as_object_mut()
        .unwrap()
        .remove(&source_key)
        .unwrap();
    entry["path"] = Value::String("fixture.ts".to_string());
    coverage
        .as_object_mut()
        .unwrap()
        .insert("fixture.ts".to_string(), entry);
    fs::write(&coverage_path, serde_json::to_vec(&coverage).unwrap()).unwrap();
    let basename = run(&fixture, &[]);
    assert_eq!(basename.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&basename.stderr).contains("missing coverage evidence"));
}

#[test]
fn malformed_counts_and_positions_fail_even_in_report_only_mode() {
    let mutations = [
        ("negative count", json!(-1), false),
        ("nonnumeric count", json!("one"), false),
        ("zero line", json!(0), true),
        ("out of range column", json!(999), true),
    ];
    for (label, value, position) in mutations {
        let fixture = Fixture::new();
        let coverage_path = fixture.root.join("coverage-final.json");
        let mut coverage: Value =
            serde_json::from_slice(&fs::read(&coverage_path).unwrap()).expect("fixture coverage");
        if position {
            coverage[fixture
                .root
                .join("src/fixture.ts")
                .to_string_lossy()
                .as_ref()]["statementMap"]["0"]
                [if label == "zero line" { "start" } else { "end" }][if label
                == "zero line"
            {
                "line"
            } else {
                "column"
            }] = value;
        } else {
            coverage[fixture
                .root
                .join("src/fixture.ts")
                .to_string_lossy()
                .as_ref()]["s"]["0"] = value;
        }
        fs::write(&coverage_path, serde_json::to_vec(&coverage).unwrap()).unwrap();
        let output = run(&fixture, &["--report-only"]);
        assert_eq!(output.status.code(), Some(1), "{label}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("coverage parsing failed"),
            "{label}"
        );
    }
}

#[test]
fn malformed_path_field_fails_even_when_the_key_is_valid() {
    let fixture = Fixture::new();
    let coverage_path = fixture.root.join("coverage-final.json");
    let source_key = fixture
        .root
        .join("src/fixture.ts")
        .to_string_lossy()
        .to_string();
    let mut coverage: Value =
        serde_json::from_slice(&fs::read(&coverage_path).unwrap()).expect("fixture coverage");
    coverage[&source_key]["path"] = json!(42);
    fs::write(&coverage_path, serde_json::to_vec(&coverage).unwrap()).unwrap();
    let output = run(&fixture, &["--report-only"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("coverage parsing failed"));
}

#[test]
fn report_only_emits_structured_unmatched_diagnostics_in_text_and_json() {
    let fixture = Fixture::new();
    let coverage_path = fixture.root.join("coverage-final.json");
    let source_key = fixture
        .root
        .join("src/fixture.ts")
        .to_string_lossy()
        .to_string();
    let mut coverage: Value =
        serde_json::from_slice(&fs::read(&coverage_path).unwrap()).expect("fixture coverage");
    coverage[&source_key]["fnMap"]["0"]["name"] = json!("stale");
    coverage[&source_key]["fnMap"]["0"]["loc"] = json!({
        "start": {"line": 1, "column": 0},
        "end": {"line": 1, "column": 1}
    });
    fs::write(&coverage_path, serde_json::to_vec(&coverage).unwrap()).unwrap();

    let text = run(&fixture, &["--report-only"]);
    assert_eq!(text.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&text.stdout).contains("diagnostic [coverage_attribution]"));

    let json_output = run(&fixture, &["--format", "json", "--report-only"]);
    assert_eq!(json_output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&json_output.stdout).expect("valid JSON report");
    assert!(report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|diagnostic| { diagnostic["category"] == "coverage_attribution" }));
}

#[test]
fn json_errors_emit_structured_ambiguity_diagnostics_on_stderr() {
    let fixture = Fixture::new();
    let coverage_path = fixture.root.join("coverage-final.json");
    let source_key = fixture
        .root
        .join("src/fixture.ts")
        .to_string_lossy()
        .to_string();
    let mut coverage: Value =
        serde_json::from_slice(&fs::read(&coverage_path).unwrap()).expect("fixture coverage");
    let duplicate = coverage[&source_key]["fnMap"]["0"].clone();
    coverage[&source_key]["fnMap"]["1"] = duplicate;
    coverage[&source_key]["f"]["1"] = json!(0);
    fs::write(&coverage_path, serde_json::to_vec(&coverage).unwrap()).unwrap();

    let output = run(&fixture, &["--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostics: Value =
        serde_json::from_slice(&output.stderr).expect("structured JSON diagnostics");
    assert_eq!(
        diagnostics["diagnostics"][0]["category"],
        "coverage_attribution"
    );
    assert!(diagnostics["diagnostics"][0]["message"]
        .as_str()
        .unwrap()
        .contains("coverage attribution failed"));
}

#[test]
fn built_in_test_file_exclusion_keeps_reportable_sources_only() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("src/ignored.test.ts"),
        "function ignored() { return 1; }\n",
    )
    .unwrap();
    fs::create_dir_all(fixture.root.join("src/tests")).unwrap();
    fs::write(
        fixture.root.join("src/tests/ignored.ts"),
        "function ignoredDirectory() { return 1; }\n",
    )
    .unwrap();
    let output = run_with_source(&fixture, &["--format", "json"], &["src"]);
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["rows"].as_array().unwrap().len(), 1);
    assert_eq!(report["rows"][0]["name"], "greet");

    let excluded_root = run_with_source(&fixture, &[], &["src/tests"]);
    assert_eq!(excluded_root.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&excluded_root.stderr)
        .contains("source selection produced no TypeScript files"));
}

#[test]
fn directory_discovery_finds_tsx_and_deduplicates_source_roots() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("src/component.tsx"),
        "export const Component = () => <span />;\n",
    )
    .unwrap();
    fs::create_dir_all(fixture.root.join("lib")).unwrap();
    fs::write(
        fixture.root.join("lib/helper.ts"),
        "export function helper() { return 1; }\n",
    )
    .unwrap();

    let output = run_with_source(
        &fixture,
        &[
            "--format",
            "json",
            "--report-only",
            "--source",
            "src",
            "--source-root",
            "src",
            "--source",
            "lib",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let paths = report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        paths.len(),
        3,
        "duplicate source roots must not duplicate rows"
    );
    let mut sorted_paths = paths.clone();
    sorted_paths.sort_unstable();
    assert_eq!(
        sorted_paths,
        ["lib/helper.ts", "src/component.tsx", "src/fixture.ts"]
    );
}

#[test]
fn missing_and_empty_source_selections_are_explicit_errors() {
    let fixture = Fixture::new();
    let missing = run_with_source(&fixture, &[], &["does-not-exist"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("unable to resolve source"));

    fs::create_dir(fixture.root.join("empty")).unwrap();
    let empty = run_with_source(&fixture, &[], &["empty"]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&empty.stderr).contains("source selection produced no"));
}

#[test]
fn explicitly_selected_unsupported_and_excluded_files_are_errors() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("src/not-typescript.js"),
        "function nope() {}\n",
    )
    .unwrap();
    let unsupported = run_with_source(&fixture, &[], &["src/not-typescript.js"]);
    assert_eq!(unsupported.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&unsupported.stderr).contains("unsupported extension"));

    fs::write(
        fixture.root.join("src/explicit.test.ts"),
        "function ignored() { return 1; }\n",
    )
    .unwrap();
    let excluded = run_with_source(&fixture, &[], &["src/explicit.test.ts"]);
    assert_eq!(excluded.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&excluded.stderr).contains("is excluded"));
}

#[test]
fn parent_and_absolute_source_escape_attempts_are_rejected() {
    let fixture = Fixture::new();
    let outside_name = format!("crap4ts-outside-{}.ts", std::process::id());
    let outside = fixture.root.parent().unwrap().join(&outside_name);
    fs::write(&outside, "function outside() { return 1; }\n").unwrap();

    let relative_name = format!("../{outside_name}");
    let relative = run_with_source(&fixture, &[], &[relative_name.as_str()]);
    assert_eq!(relative.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&relative.stderr).contains("escapes project root"));

    let absolute = run_with_source(
        &fixture,
        &[],
        &[outside.to_str().expect("UTF-8 fixture path")],
    );
    assert_eq!(absolute.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&absolute.stderr).contains("escapes project root"));
    let _ = fs::remove_file(outside);
}

#[test]
fn issue_three_tsx_fixture_reports_all_units_and_complexity() {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "crap4ts-cli-issue-three-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("src")).expect("create fixture");
    fs::write(
        root.join("src/issue-three.tsx"),
        include_str!("fixtures/issue-three.tsx"),
    )
    .expect("write source");
    fs::write(root.join("coverage-final.json"), b"{}").expect("write coverage");

    let output = Command::new(binary())
        .current_dir(&root)
        .args([
            "--coverage",
            "coverage-final.json",
            "--format",
            "json",
            "--report-only",
            "src/issue-three.tsx",
        ])
        .output()
        .expect("run crap4ts");
    assert_eq!(output.status.code(), Some(0));
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("valid JSON report");
    let rows = report["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 18);
    assert!(rows
        .iter()
        .all(|row| row["coverage"]["status"] == "unknown"));

    let assert_row = |name: &str, kind: &str, complexity: u64| {
        let row = rows
            .iter()
            .find(|row| row["name"] == name && row["kind"] == kind)
            .unwrap_or_else(|| panic!("missing {kind} {name}"));
        assert_eq!(row["complexity"], complexity, "{kind} {name}");
    };
    assert_row("Component", "arrow", 1);
    assert_row("onClick", "arrow", 1);
    assert_row("declaration", "function_declaration", 3);
    assert_row("expression", "function_expression", 1);
    assert_row("method", "method", 2);
    assert_row("getter", "getter", 1);
    assert_row("setter", "setter", 1);
    assert_row("expressionProperty", "function_expression", 1);
    assert_row("arrowProperty", "arrow", 1);
    assert_row("constructor", "constructor", 1);
    assert_row("value", "getter", 1);
    assert_row("value", "setter", 1);
    assert_row("field", "arrow", 1);
    assert_row("decisions", "function_declaration", 18);
    assert_row("asynchronous", "function_declaration", 1);
    assert_row("generator", "function_declaration", 1);
    assert_row("overloaded", "function_declaration", 1);
    assert!(!rows.iter().any(|row| row["name"] == "ambient"));
    assert!(!rows.iter().any(|row| row["name"] == "run"));

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn symlink_directory_cycle_is_rejected_without_recursing() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink(&fixture.root, fixture.root.join("src/cycle")).unwrap();
    let output = run_with_source(&fixture, &[], &["."]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("symlink directory"));
}

#[cfg(unix)]
#[test]
fn symlink_source_escape_is_rejected_during_directory_discovery() {
    let fixture = Fixture::new();
    let outside = fixture
        .root
        .parent()
        .unwrap()
        .join(format!("crap4ts-symlink-outside-{}.ts", std::process::id()));
    fs::write(&outside, "function outside() { return 1; }\n").unwrap();
    std::os::unix::fs::symlink(&outside, fixture.root.join("src/escaped.ts")).unwrap();

    let output = run_with_source(&fixture, &[], &["src"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("escapes selected source root"),
        "unexpected stderr: {stderr}"
    );
    let _ = fs::remove_file(outside);
}

#[cfg(unix)]
#[test]
fn selected_directory_is_a_symlink_confinement_boundary() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("other")).unwrap();
    fs::write(
        fixture.root.join("other/escaped.ts"),
        "function escaped() { return 1; }\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        fixture.root.join("other/escaped.ts"),
        fixture.root.join("src/escaped.ts"),
    )
    .unwrap();

    let output = run_with_source(&fixture, &[], &["src"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("selected source root"));
}

#[cfg(unix)]
#[test]
fn symlink_targets_in_excluded_directories_are_not_selected() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("src/tests")).unwrap();
    fs::write(
        fixture.root.join("src/tests/hidden.ts"),
        "function hidden() { return 1; }\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        fixture.root.join("src/tests/hidden.ts"),
        fixture.root.join("src/alias.ts"),
    )
    .unwrap();

    let output = run_with_source(&fixture, &["--format", "json"], &["src"]);
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["rows"].as_array().unwrap().len(), 1);
    assert_eq!(report["rows"][0]["path"], "src/fixture.ts");
}

#[test]
fn project_config_supplies_values_and_explicit_cli_wins() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("crap4ts.json"),
        serde_json::to_vec(&json!({
            "sources": ["src/fixture.ts"],
            "coverage": "coverage-final.json",
            "format": "json",
            "threshold": 0
        }))
        .unwrap(),
    )
    .unwrap();

    let configured = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--config")
        .arg("crap4ts.json")
        .output()
        .expect("run configured crap4ts");
    assert_eq!(configured.status.code(), Some(2));
    let configured_report: Value =
        serde_json::from_slice(&configured.stdout).expect("configured JSON report");
    assert_eq!(configured_report["version"], 1);
    assert_eq!(configured_report["threshold"], 0);
    assert!(String::from_utf8_lossy(&configured.stderr).contains("quality gate breached"));

    let overridden = Command::new(binary())
        .current_dir(&fixture.root)
        .args(["--config", "crap4ts.json", "--threshold", "1"])
        .output()
        .expect("run CLI-overridden crap4ts");
    assert_eq!(overridden.status.code(), Some(0));
    let overridden_report: Value =
        serde_json::from_slice(&overridden.stdout).expect("overridden JSON report");
    assert_eq!(overridden_report["threshold"], 1);
    assert!(overridden.stderr.is_empty());
}

fn mixed_groups_config() -> Value {
    json!({
        "lcov": {
            "root": "packages/lcov",
            "sources": ["src"],
            "coverage": {"path": "lcov.info", "format": "lcov"},
            "threshold": 2
        },
        "istanbul": {
            "root": "packages/istanbul",
            "sources": ["src"],
            "coverage": {"path": "coverage-final.json", "format": "istanbul"},
            "threshold": 1
        }
    })
}

#[test]
fn mixed_package_groups_are_analyzed_with_independent_identity_and_policy() {
    let fixture = MixedFixture::new();
    fixture.config(mixed_groups_config());
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run mixed workspace");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("aggregate JSON report");
    assert_eq!(report["version"], 2);
    assert_eq!(
        report["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|group| group["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["istanbul", "lcov"]
    );
    assert_eq!(report["groups"][0]["threshold"], 1);
    assert_eq!(report["groups"][1]["threshold"], 2);
    let rows = report["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row["group"].is_string()));
    assert!(rows
        .iter()
        .all(|row| row["path"].as_str().unwrap().starts_with("packages/")));
    assert_ne!(rows[0]["id"], rows[1]["id"]);
    assert!(rows
        .iter()
        .any(|row| row["group"] == "istanbul" && row["coverage"]["fraction"] == 1.0));
    assert!(rows
        .iter()
        .any(|row| row["group"] == "lcov" && row["coverage"]["fraction"] == 1.0));

    let repeat = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("repeat mixed workspace");
    assert_eq!(repeat.status.code(), Some(0));
    assert_eq!(output.stdout, repeat.stdout);
    assert_eq!(output.stderr, repeat.stderr);
}

#[test]
fn checked_in_issue_nine_fixture_covers_istanbul_and_lcov_groups() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/issue-nine");
    let output = Command::new(binary())
        .current_dir(root)
        .args(["--format", "json"])
        .output()
        .expect("run checked-in mixed fixture");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("fixture JSON report");
    assert_eq!(report["version"], 2);
    assert_eq!(report["rows"].as_array().unwrap().len(), 2);
    assert!(report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["coverage"]["status"] == "measured"));
}

#[test]
fn package_group_declaration_order_does_not_change_canonical_json() {
    let fixture = MixedFixture::new();
    let first = json!([
        {
            "name": "lcov",
            "root": "packages/lcov",
            "sources": ["src"],
            "coverage": {"path": "lcov.info", "format": "lcov"},
            "threshold": 2
        },
        {
            "name": "istanbul",
            "root": "packages/istanbul",
            "sources": ["src"],
            "coverage": {"path": "coverage-final.json", "format": "istanbul"},
            "threshold": 1
        }
    ]);
    fixture.config(first);
    let first_output = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run first declaration order");
    assert_eq!(first_output.status.code(), Some(0));

    let second = json!([
        {
            "name": "istanbul",
            "root": "packages/istanbul",
            "sources": ["src"],
            "coverage": {"path": "coverage-final.json", "format": "istanbul"},
            "threshold": 1
        },
        {
            "name": "lcov",
            "root": "packages/lcov",
            "sources": ["src"],
            "coverage": {"path": "lcov.info", "format": "lcov"},
            "threshold": 2
        }
    ]);
    fixture.config(second);
    let second_output = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run second declaration order");
    assert_eq!(second_output.status.code(), Some(0));
    assert_eq!(first_output.stdout, second_output.stdout);
}

#[test]
fn package_group_cli_analysis_overrides_are_rejected_instead_of_broadcast() {
    let fixture = MixedFixture::new();
    fixture.config(mixed_groups_config());
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .args(["--threshold", "0", "--format", "json"])
        .output()
        .expect("run rejected group override");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be used with groups"));
}

#[test]
fn package_group_root_must_remain_inside_repository_root() {
    let fixture = MixedFixture::new();
    fixture.config(json!({
        "escape": {
            "root": "..",
            "sources": ["src"],
            "coverage": {"path": "coverage.json"}
        }
    }));
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run escaped group");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("parent traversal"));
}

#[test]
fn package_group_failure_has_no_partial_stdout_and_breach_does_not_mask_failure() {
    let fixture = MixedFixture::new();
    let mut groups = mixed_groups_config();
    groups["istanbul"]["threshold"] = json!(0);
    groups["lcov"]["coverage"] = json!("does-not-exist.info");
    fixture.config(groups);
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run failed mixed workspace");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(r#""group":"lcov""#), "stderr: {stderr}");
    assert!(!stderr.contains("quality gate breached"));
}

#[test]
fn overlapping_group_sources_and_artifacts_are_rejected_during_preflight() {
    let fixture = MixedFixture::new();
    let groups = json!({
        "one": {
            "root": "packages/istanbul",
            "sources": ["src"],
            "coverage": {"path": "coverage-final.json"}
        },
        "two": {
            "root": "packages/istanbul",
            "sources": ["src"],
            "coverage": {"path": "coverage-final.json"}
        }
    });
    fixture.config(groups);
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run overlapping groups");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overlaps package group"));
}

#[cfg(unix)]
#[test]
fn later_group_preflight_failure_runs_no_earlier_generation_command() {
    let fixture = MixedFixture::new();
    let marker = "packages/istanbul/generated.marker";
    let groups = json!({
        "first": {
            "root": "packages/istanbul",
            "sources": ["src"],
            "coverage": {
                "path": "generated.json",
                "command": ["sh", "-c", "touch generated.marker; cp coverage-final.json generated.json"]
            }
        },
        "later": {
            "root": "packages/lcov",
            "sources": ["missing"],
            "coverage": {"path": "lcov.info", "format": "lcov"}
        }
    });
    fixture.config(groups);
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run preflight failure");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!fixture.root.join(marker).exists());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(r#""group":"later""#),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn malformed_existing_group_artifact_is_preflighted_before_generation() {
    let fixture = MixedFixture::new();
    fs::write(
        fixture.root.join("packages/lcov/lcov.info"),
        "this is not an LCOV tracefile\n",
    )
    .expect("replace later artifact");
    let groups = json!({
        "first": {
            "root": "packages/istanbul",
            "sources": ["src"],
            "coverage": {
                "path": "generated.json",
                "command": ["sh", "-c", "touch generated.marker; cp coverage-final.json generated.json"]
            }
        },
        "later": {
            "root": "packages/lcov",
            "sources": ["src"],
            "coverage": {"path": "lcov.info", "format": "lcov"}
        }
    });
    fixture.config(groups);
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .arg("--format")
        .arg("json")
        .output()
        .expect("run malformed preflight");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!fixture
        .root
        .join("packages/istanbul/generated.marker")
        .exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains(r#""group":"later""#));
}

#[test]
fn discovered_config_is_strict_and_data_only() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("crap4ts.json"),
        serde_json::to_vec(&json!({
            "coverage": "coverage-final.json",
            "sources": ["src/fixture.ts"],
            "run": "echo should never execute"
        }))
        .unwrap(),
    )
    .unwrap();
    let unknown = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run unknown-field config");
    assert_eq!(unknown.status.code(), Some(1));
    assert!(unknown.stdout.is_empty());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown field"));

    fs::remove_file(fixture.root.join("crap4ts.json")).unwrap();
    fs::write(
        fixture.root.join("crap4ts.config.js"),
        "module.exports = { coverage: 'coverage-final.json' };",
    )
    .unwrap();
    let executable = Command::new(binary())
        .current_dir(&fixture.root)
        .args(["--config", "crap4ts.config.js"])
        .output()
        .expect("run executable config");
    assert_eq!(executable.status.code(), Some(1));
    assert!(executable.stdout.is_empty());
    assert!(String::from_utf8_lossy(&executable.stderr).contains("executable config"));
}

#[test]
fn config_rejects_duplicate_keys_and_explicit_nulls() {
    let fixture = Fixture::new();
    let config_path = fixture.root.join("crap4ts.json");
    fs::write(
        &config_path,
        br#"{"coverage":"coverage-final.json","thresholds":{"src/fixture.ts":1,"src/fixture.ts":2}}"#,
    )
    .unwrap();
    let duplicate = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run duplicate-key config");
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("duplicate JSON object key"));

    fs::write(
        &config_path,
        br#"{"coverage":"coverage-final.json","threshold":null}"#,
    )
    .unwrap();
    let null = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run null config");
    assert_eq!(null.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&null.stderr).contains("null is not permitted"));
}

#[test]
fn config_report_only_retains_unknown_rows_without_a_score() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("coverage-final.json"), b"{}").unwrap();
    fs::write(
        fixture.root.join("crap4ts.json"),
        serde_json::to_vec(&json!({
            "coverage": "coverage-final.json",
            "sources": ["src/fixture.ts"],
            "report": {"format": "json"},
            "missing_evidence": "report_only"
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run report-only config");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(report["rows"][0]["coverage"]["status"], "unknown");
    assert!(report["rows"][0]["crap"].is_null());
}

#[test]
fn path_threshold_override_uses_normalized_project_identity() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("crap4ts.json"),
        serde_json::to_vec(&json!({
            "coverage": "coverage-final.json",
            "sources": ["src/fixture.ts"],
            "format": "json",
            "threshold": 0,
            "thresholds": {"src\\fixture.ts": 1}
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run path-threshold config");
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(report["rows"][0]["path"], "src/fixture.ts");
    assert_eq!(report["rows"][0]["crap"], 1.0);
}

#[cfg(unix)]
fn generated_config(fixture: &Fixture, coverage: Value, command: Value) {
    fs::write(
        fixture.root.join("crap4ts.json"),
        serde_json::to_vec(&json!({
            "sources": ["src/fixture.ts"],
            "coverage": {
                "path": coverage,
                "format": "istanbul",
                "command": command
            },
            "format": "json"
        }))
        .unwrap(),
    )
    .unwrap();
}

#[cfg(unix)]
fn shell_command(script: &str) -> Value {
    json!(["sh", "-c", script])
}

#[cfg(unix)]
#[test]
fn generated_mode_removes_stale_artifact_and_accepts_same_bytes_as_fresh() {
    let fixture = Fixture::new();
    fs::copy(
        fixture.root.join("coverage-final.json"),
        fixture.root.join("coverage-template.json"),
    )
    .unwrap();
    fs::write(fixture.root.join("coverage-final.json"), b"stale").unwrap();
    generated_config(
        &fixture,
        json!("coverage-final.json"),
        shell_command("cp coverage-template.json coverage-final.json"),
    );

    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run generated analysis");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
    assert_eq!(report["rows"][0]["coverage"]["status"], "measured");
}

#[cfg(unix)]
#[test]
fn cli_generation_command_preserves_program_and_argument_boundaries() {
    let fixture = Fixture::new();
    fs::copy(
        fixture.root.join("coverage-final.json"),
        fixture.root.join("coverage-template.json"),
    )
    .unwrap();
    fs::write(fixture.root.join("coverage-final.json"), b"stale").unwrap();
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .args([
            "--coverage",
            "coverage-final.json",
            "src/fixture.ts",
            "--format",
            "json",
            "--coverage-command",
            "cp",
            "--coverage-arg",
            "coverage-template.json",
            "--coverage-arg",
            "coverage-final.json",
        ])
        .output()
        .expect("run CLI-configured generation");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["version"],
        1
    );
}

#[cfg(unix)]
#[test]
fn generated_mode_stops_on_command_failure_and_never_reuses_stale_or_partial_output() {
    let fixture = Fixture::new();
    generated_config(
        &fixture,
        json!("coverage-final.json"),
        shell_command("printf 'command output\\n'; exit 7"),
    );
    let failed = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run failed generation");
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("command output"));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("coverage command failed"));
    assert!(!fixture.root.join("coverage-final.json").exists());

    generated_config(
        &fixture,
        json!("coverage-final.json"),
        shell_command("printf partial > coverage-final.json; exit 9"),
    );
    let partial = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run partial failed generation");
    assert_eq!(partial.status.code(), Some(1));
    assert!(partial.stdout.is_empty());
    assert_eq!(
        fs::read(fixture.root.join("coverage-final.json")).unwrap(),
        b"partial"
    );
}

#[cfg(unix)]
#[test]
fn generated_mode_requires_a_new_readable_artifact() {
    let fixture = Fixture::new();
    generated_config(&fixture, json!("coverage-final.json"), shell_command(":"));
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run missing artifact generation");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("did not produce fresh"));
    assert!(!fixture.root.join("coverage-final.json").exists());
}

#[cfg(unix)]
#[test]
fn generated_child_output_is_raw_stderr_and_cannot_corrupt_json_stdout() {
    let fixture = Fixture::new();
    fs::copy(
        fixture.root.join("coverage-final.json"),
        fixture.root.join("coverage-template.json"),
    )
    .unwrap();
    generated_config(
        &fixture,
        json!("coverage-final.json"),
        shell_command(
            "head -c 131072 /dev/zero; printf '\\377child-stderr\\n' >&2; cp coverage-template.json coverage-final.json",
        ),
    );
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run noisy generation");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON stdout remains valid");
    assert_eq!(report["version"], 1);
    assert!(output.stderr.windows(2).any(|bytes| bytes == b"\0\0"));
    assert!(output
        .stderr
        .windows(13)
        .any(|bytes| bytes == b"child-stderr\n"));
}

#[cfg(unix)]
#[test]
fn generated_mode_rejects_traversal_root_directories_and_symlink_escapes() {
    let fixture = Fixture::new();
    let outside = fixture
        .root
        .parent()
        .unwrap()
        .join(format!("crap4ts-generated-outside-{}", std::process::id()));
    fs::write(&outside, b"outside sentinel").unwrap();
    let outside_name = outside.file_name().unwrap().to_string_lossy().into_owned();
    generated_config(
        &fixture,
        json!(format!("../{outside_name}")),
        shell_command(&format!("printf should-not-run > ../{outside_name}")),
    );
    let traversal = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run traversal generation");
    assert_eq!(traversal.status.code(), Some(1));
    assert!(traversal.stdout.is_empty());
    assert!(String::from_utf8_lossy(&traversal.stderr).contains("unsafe coverage artifact path"));
    assert_eq!(fs::read(&outside).unwrap(), b"outside sentinel");

    generated_config(&fixture, json!("."), shell_command("true"));
    let root = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run root generation");
    assert_eq!(root.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&root.stderr).contains("unsafe coverage artifact path"));

    fs::create_dir(fixture.root.join("artifact-dir")).unwrap();
    generated_config(&fixture, json!("artifact-dir"), shell_command("true"));
    let directory = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run directory generation");
    assert_eq!(directory.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&directory.stderr).contains("must be a regular file"));

    fs::create_dir(fixture.root.join("outside-parent")).unwrap();
    std::os::unix::fs::symlink(
        fixture.root.join("outside-parent"),
        fixture.root.join("generated-parent"),
    )
    .unwrap();
    generated_config(
        &fixture,
        json!("generated-parent/coverage-final.json"),
        shell_command("touch generated-parent/coverage-final.json"),
    );
    let symlink_parent = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run symlink-parent generation");
    assert_eq!(symlink_parent.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&symlink_parent.stderr).contains("symlink ancestor"));
    assert!(!fixture
        .root
        .join("outside-parent/coverage-final.json")
        .exists());

    fs::remove_file(fixture.root.join("coverage-final.json")).unwrap();
    std::os::unix::fs::symlink(&outside, fixture.root.join("coverage-final.json")).unwrap();
    generated_config(
        &fixture,
        json!("coverage-final.json"),
        shell_command("cp coverage-template.json coverage-final.json"),
    );
    let symlink_target = Command::new(binary())
        .current_dir(&fixture.root)
        .output()
        .expect("run symlink-target generation");
    assert_eq!(symlink_target.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&symlink_target.stderr).contains("symlink"));
    assert!(
        fs::symlink_metadata(fixture.root.join("coverage-final.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&outside).unwrap(), b"outside sentinel");
    let _ = fs::remove_file(&outside);
}

#[cfg(unix)]
#[test]
fn existing_artifact_mode_never_runs_configured_command_or_cleans_artifact() {
    let fixture = Fixture::new();
    let original = fs::read(fixture.root.join("coverage-final.json")).unwrap();
    generated_config(
        &fixture,
        json!("coverage-final.json"),
        json!(["command-that-does-not-exist", "--would-fail"]),
    );
    let output = Command::new(binary())
        .current_dir(&fixture.root)
        .args(["--no-generate"])
        .output()
        .expect("run existing-artifact mode");
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(fixture.root.join("coverage-final.json")).unwrap(),
        original
    );
    assert!(output.stderr.is_empty());
}
