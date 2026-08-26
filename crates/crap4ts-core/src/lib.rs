//! Library-neutral core for the crap4ts quality gate.
//!
//! Module boundaries keep filesystem selection, Oxc source analysis, Istanbul
//! normalization, scoring policy, and report rendering independent. Only
//! normalized domain values cross those boundaries.

mod application;
mod coverage;
mod domain;
mod path;
mod report;
mod source;

pub use application::{analyze, analyze_with_root, crap_score};
pub use domain::{
    Complexity, CoreError, Coverage, Diagnostic, DiagnosticCategory, FunctionKind, FunctionUnit,
    ProjectRelativePath, Report, ReportRow, SourceFile, SourcePosition, SourceRange,
    REPORT_VERSION,
};
pub use path::collect_sources;
pub use report::{render_json, render_text};

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn position(line: usize, column: usize) -> serde_json::Value {
        json!({ "line": line, "column": column })
    }

    fn range(range: SourceRange) -> serde_json::Value {
        json!({
            "start": position(range.start.line, range.start.column),
            "end": position(range.end.line, range.end.column)
        })
    }

    #[test]
    fn complexity_does_not_include_nested_function_body() {
        let source = "function outer() { if (true) { return () => { if (false) return 1; }; } }";
        let path = ProjectRelativePath::new("fixture.ts").unwrap();
        let units = super::source::analyze_source(&path, source).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].name, "outer");
        assert_eq!(units[0].complexity.get(), 2);
        assert_eq!(units[1].kind, FunctionKind::Arrow);
        assert_eq!(units[1].complexity.get(), 2);
    }

    #[test]
    fn methods_and_accessors_are_distinct_units() {
        let source = "class Counter { value() { if (true) return 1; } get current() { return 1; } set current(value) { this.value = value; } }";
        let path = ProjectRelativePath::new("fixture.ts").unwrap();
        let units = super::source::analyze_source(&path, source).unwrap();
        assert_eq!(units.len(), 3);
        assert_eq!(units[0].kind, FunctionKind::Method);
        assert_eq!(units[0].name, "value");
        assert_eq!(units[1].kind, FunctionKind::Getter);
        assert_eq!(units[1].name, "current");
        assert_eq!(units[2].kind, FunctionKind::Setter);
        assert_eq!(units[2].name, "current");
    }

    #[test]
    fn nested_same_names_use_exact_function_ranges() {
        let source = "function same() { function same() { return 1; } return same(); }\n";
        let path = ProjectRelativePath::new("fixture.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let units = super::source::analyze_source(&path, source).unwrap();
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].name, "same");
        assert_eq!(units[1].name, "same");
        let coverage = json!({
            "fixture.ts": {
                "fnMap": {
                    "0": {"name": "same", "loc": range(units[0].body_range)},
                    "1": {"name": "same", "loc": range(units[1].body_range)}
                },
                "f": {"0": 0, "1": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            false,
        )
        .unwrap();
        let parent = report
            .rows
            .iter()
            .find(|row| row.id == units[0].id)
            .unwrap();
        let child = report
            .rows
            .iter()
            .find(|row| row.id == units[1].id)
            .unwrap();
        assert_eq!(parent.coverage, Coverage::measured(0, 1).unwrap());
        assert_eq!(child.coverage, Coverage::measured(1, 1).unwrap());
    }

    #[test]
    fn ambiguous_duplicate_function_ranges_fail_closed() {
        let source = "function same() { return 1; }\n";
        let path = ProjectRelativePath::new("fixture.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let unit = super::source::analyze_source(&path, source)
            .unwrap()
            .pop()
            .unwrap();
        let coverage = json!({
            "fixture.ts": {
                "fnMap": {
                    "0": {"name": "same", "loc": range(unit.body_range)},
                    "1": {"name": "same", "loc": range(unit.body_range)}
                },
                "f": {"0": 0, "1": 1}
            }
        });
        let error = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            true,
        )
        .unwrap_err();
        assert!(matches!(error, CoreError::CoverageAttribution(_)));
    }

    #[test]
    fn golden_crap_vectors() {
        assert_eq!(crap_score(Complexity::one(), 1.0).unwrap(), 1.0);
        assert_eq!(crap_score(Complexity::one(), 0.0).unwrap(), 2.0);
        assert_eq!(crap_score(Complexity::new(2).unwrap(), 0.5).unwrap(), 2.5);
    }

    #[test]
    fn project_paths_reject_absolute_and_parent_inputs() {
        assert!(ProjectRelativePath::new("/tmp/source.ts").is_err());
        assert!(ProjectRelativePath::new("../source.ts").is_err());
        assert!(ProjectRelativePath::new("C:/source.ts").is_err());
        assert_eq!(
            ProjectRelativePath::new("./src\\file.ts").unwrap().as_str(),
            "src/file.ts"
        );
    }
}
