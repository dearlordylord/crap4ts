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

    #[test]
    fn inspect_issue_three_units() {
        let source = r#"
const variableArrow = (value: boolean) => value ? value && value : false;
const variableFunction = function (value: number) { return value; };
const explicitFunction = function namedFunction(value: number) { return value; };
const object = {
    method() { if (true) return 1; },
    get getter() { return 1; },
    set setter(value: number) { this.value = value; },
    functionProperty: function (value: number) { return value; },
    arrowProperty: (value: number) => value ? value : 0,
    [(() => {})]() { return 1; },
};
class Example {
    constructor() { return; }
    static method() { return 1; }
    get value() { return 1; }
    set value(next: number) { this.next = next; }
    field = () => 1;
    ["computed"]() { return 1; }
}
function outer() {
    if (true) {
        const callback = (value: boolean) => {
            if (value) return value && value;
            return false;
        };
        return callback;
    }
    return undefined;
}
function decisions(value: any) {
    if (value) return;
    for (let index = 0; index < 1; index += 1) {}
    for (const key in value) {}
    for (const item of value) {}
    while (value) break;
    do { break; } while (value);
    try { return value; } catch (error) { return error; }
    switch (value) { case 1: break; case 2: break; default: break; }
    const conditional = value ? value : 0;
    value &&= value;
    value ||= value;
    value ??= value;
    return value && value || (value ?? value);
}
async function asyncFunction() { return 1; }
function* generator() { yield 1; }
function overload(value: string): string;
function overload(value: number): number;
function overload(value: unknown): unknown { return value; }
declare function ambient(): void;
interface Interface { run(): void; }
abstract class Abstract { abstract run(): void; }
        "#;
        let path = ProjectRelativePath::new("fixture.ts").unwrap();
        let units = super::source::analyze_source(&path, source).unwrap();
        let summary: Vec<(&str, FunctionKind, u32)> = units
            .iter()
            .map(|unit| (unit.name.as_str(), unit.kind, unit.complexity.get()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("variableArrow", FunctionKind::Arrow, 3),
                ("variableFunction", FunctionKind::FunctionExpression, 1),
                ("namedFunction", FunctionKind::FunctionExpression, 1),
                ("method", FunctionKind::Method, 2),
                ("getter", FunctionKind::Getter, 1),
                ("setter", FunctionKind::Setter, 1),
                ("functionProperty", FunctionKind::FunctionExpression, 1),
                ("arrowProperty", FunctionKind::Arrow, 2),
                ("<anonymous>@11:6", FunctionKind::Arrow, 1),
                ("<anonymous>@11:16", FunctionKind::Method, 1),
                ("constructor", FunctionKind::Constructor, 1),
                ("method", FunctionKind::Method, 1),
                ("value", FunctionKind::Getter, 1),
                ("value", FunctionKind::Setter, 1),
                ("field", FunctionKind::Arrow, 1),
                ("computed", FunctionKind::Method, 1),
                ("outer", FunctionKind::FunctionDeclaration, 2),
                ("callback", FunctionKind::Arrow, 3),
                ("decisions", FunctionKind::FunctionDeclaration, 17),
                ("asyncFunction", FunctionKind::FunctionDeclaration, 1),
                ("generator", FunctionKind::FunctionDeclaration, 1),
                ("overload", FunctionKind::FunctionDeclaration, 1),
            ]
        );
        assert_eq!(units.len(), 22);
    }

    #[test]
    fn tsx_generics_decorators_and_nested_jsx_callbacks_are_structural() {
        let source = r#"
type Item<T> = { value: T };
const Component = <T,>(props: Item<T>) => (
    <section onClick={() => props.value}>{props.value}</section>
);
function sealed() { return () => 1; }
@sealed
class Decorated {
    @sealed
    async *method() { yield 1; }
}
"#;
        let path = ProjectRelativePath::new("fixture.tsx").unwrap();
        let units = super::source::analyze_source(&path, source).unwrap();
        let summary: Vec<(&str, FunctionKind, u32)> = units
            .iter()
            .map(|unit| (unit.name.as_str(), unit.kind, unit.complexity.get()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Component", FunctionKind::Arrow, 1),
                ("onClick", FunctionKind::Arrow, 1),
                ("sealed", FunctionKind::FunctionDeclaration, 1),
                ("<anonymous>@6:27", FunctionKind::Arrow, 1),
                ("method", FunctionKind::Method, 1),
            ]
        );
    }

    #[test]
    fn inferred_names_cover_wrappers_destructuring_and_assignments() {
        let source = r#"
const wrapped = ((value: number) => value) as (value: number) => number;
const named = function explicit() { return 1; };
const { defaulted = () => 1 } = {};
function parameters(callback = () => 1) { return callback; }
let assigned;
assigned = () => 1;
const object = { ["literal"]: () => 1 };
object.member = function () { return 1; };
"#;
        let path = ProjectRelativePath::new("fixture.ts").unwrap();
        let units = super::source::analyze_source(&path, source).unwrap();
        let summary: Vec<(&str, FunctionKind)> = units
            .iter()
            .map(|unit| (unit.name.as_str(), unit.kind))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("wrapped", FunctionKind::Arrow),
                ("explicit", FunctionKind::FunctionExpression),
                ("defaulted", FunctionKind::Arrow),
                ("parameters", FunctionKind::FunctionDeclaration),
                ("callback", FunctionKind::Arrow),
                ("assigned", FunctionKind::Arrow),
                ("literal", FunctionKind::Arrow),
                ("member", FunctionKind::FunctionExpression),
            ]
        );
    }

    #[test]
    fn class_auto_accessor_arrow_initializer_is_named() {
        let source = "class Example { accessor callback = () => 1; }\n";
        let path = ProjectRelativePath::new("fixture.ts").unwrap();
        let units = super::source::analyze_source(&path, source).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "callback");
        assert_eq!(units[0].kind, FunctionKind::Arrow);
        assert_eq!(units[0].complexity, Complexity::one());
    }
}
