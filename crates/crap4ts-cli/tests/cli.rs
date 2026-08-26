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
