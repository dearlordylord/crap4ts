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

pub use application::{
    analyze, analyze_with_policy, analyze_with_root, crap_score, ThresholdPolicy,
};
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

    fn istanbul_position(source: &str, position: SourcePosition) -> serde_json::Value {
        let line_start = source
            .bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'\n')
            .map(|(offset, _)| offset + 1)
            .chain(std::iter::once(0))
            .filter(|offset| *offset <= position.offset)
            .max()
            .unwrap_or(0);
        let line_prefix = &source[line_start..position.offset];
        json!({
            "line": position.line,
            "column": line_prefix.encode_utf16().count()
        })
    }

    fn istanbul_range(source: &str, range: SourceRange) -> serde_json::Value {
        json!({
            "start": istanbul_position(source, range.start),
            "end": istanbul_position(source, range.end)
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
            true,
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
    fn separator_only_same_line_fn_map_entry_stays_unknown() {
        let source = "const first = () => 1; const second = () => 2;\n";
        let path = ProjectRelativePath::new("separator.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let units = super::source::analyze_source(&path, source).unwrap();
        let first = units.iter().find(|unit| unit.name == "first").unwrap();
        let coverage = json!({
            "separator.ts": {
                "fnMap": {
                    "0": {
                        "name": "first",
                        "loc": {
                            "start": position(first.range.end.line, first.range.end.column),
                            "end": {"line": 1, "column": null}
                        }
                    }
                },
                "f": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            true,
        )
        .unwrap();
        let first_row = report.rows.iter().find(|row| row.id == first.id).unwrap();
        assert!(matches!(first_row.coverage, Coverage::Unknown { .. }));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.category == DiagnosticCategory::CoverageAttribution
                && diagnostic
                    .message
                    .contains("unmatched Istanbul function-map entry")
        }));
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
    fn unicode_before_function_uses_istanbul_utf16_columns() {
        let source = "const emoji = '😀'; function greet() { return emoji; }\n";
        let path = ProjectRelativePath::new("unicode.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let unit = super::source::analyze_source(&path, source)
            .unwrap()
            .into_iter()
            .find(|unit| unit.name == "greet")
            .unwrap();
        let coverage = json!({
            "unicode.ts": {
                "statementMap": {
                    "0": istanbul_range(source, unit.body_range)
                },
                "fnMap": {
                    "0": {
                        "name": "greet",
                        "loc": istanbul_range(source, unit.body_range)
                    }
                },
                "s": {"0": 1},
                "f": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            false,
        )
        .unwrap();
        let row = report.rows.iter().find(|row| row.id == unit.id).unwrap();
        assert_eq!(row.range, unit.range);
        assert_eq!(row.coverage, Coverage::measured(1, 1).unwrap());
        assert_eq!(row.crap, Some(1.0));
    }

    #[test]
    fn unmatched_nested_function_is_a_barrier_for_parent_statements() {
        let source =
            "function outer() {\n  return 1;\n  const child = () => {\n    return 2;\n  };\n}\n";
        let path = ProjectRelativePath::new("nested.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let units = super::source::analyze_source(&path, source).unwrap();
        let parent = units.iter().find(|unit| unit.name == "outer").unwrap();
        let child = units.iter().find(|unit| unit.name == "child").unwrap();
        let coverage = json!({
            "nested.ts": {
                "statementMap": {
                    "parent": {"start": position(2, 2), "end": position(2, 10)},
                    "child": {"start": position(4, 4), "end": position(4, 12)}
                },
                "fnMap": {
                    "0": {"name": "outer", "loc": range(parent.body_range)}
                },
                "s": {"parent": 1, "child": 1},
                "f": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            true,
        )
        .unwrap();
        let parent_row = report.rows.iter().find(|row| row.id == parent.id).unwrap();
        let child_row = report.rows.iter().find(|row| row.id == child.id).unwrap();
        assert_eq!(parent_row.coverage, Coverage::measured(1, 1).unwrap());
        assert!(matches!(child_row.coverage, Coverage::Unknown { .. }));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.category == DiagnosticCategory::CoverageAttribution
                && diagnostic
                    .message
                    .contains("inside an unmatched source function")
        }));
    }

    #[test]
    fn unmatched_parent_does_not_block_a_matched_nested_function() {
        let source = "function outer() {\n  const child = () => {\n    return 2;\n  };\n}\n";
        let path = ProjectRelativePath::new("nested-parent.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let units = super::source::analyze_source(&path, source).unwrap();
        let parent = units.iter().find(|unit| unit.name == "outer").unwrap();
        let child = units.iter().find(|unit| unit.name == "child").unwrap();
        let coverage = json!({
            "nested-parent.ts": {
                "statementMap": {
                    "child": {"start": {"line": 3, "column": 4}, "end": {"line": 3, "column": 12}}
                },
                "fnMap": {
                    "0": {"name": "child", "loc": range(child.body_range)}
                },
                "s": {"child": 1},
                "f": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            true,
        )
        .unwrap();
        let parent_row = report.rows.iter().find(|row| row.id == parent.id).unwrap();
        let child_row = report.rows.iter().find(|row| row.id == child.id).unwrap();
        assert!(matches!(parent_row.coverage, Coverage::Unknown { .. }));
        assert_eq!(child_row.coverage, Coverage::measured(1, 1).unwrap());
    }

    #[test]
    fn same_named_functions_in_different_files_keep_file_scoped_coverage() {
        let first_path = ProjectRelativePath::new("one.ts").unwrap();
        let second_path = ProjectRelativePath::new("two.ts").unwrap();
        let first_source = "function same() { return 1; }\n";
        let second_source = "function same() { return 2; }\n";
        let first = SourceFile {
            path: first_path.clone(),
            source: first_source.to_string(),
        };
        let second = SourceFile {
            path: second_path.clone(),
            source: second_source.to_string(),
        };
        let first_unit = super::source::analyze_source(&first_path, first_source)
            .unwrap()
            .pop()
            .unwrap();
        let second_unit = super::source::analyze_source(&second_path, second_source)
            .unwrap()
            .pop()
            .unwrap();
        let coverage = json!({
            "one.ts": {
                "statementMap": {"0": range(first_unit.body_range)},
                "fnMap": {"0": {"name": "same", "loc": range(first_unit.body_range)}},
                "s": {"0": 0},
                "f": {"0": 0}
            },
            "two.ts": {
                "statementMap": {"0": range(second_unit.body_range)},
                "fnMap": {"0": {"name": "same", "loc": range(second_unit.body_range)}},
                "s": {"0": 1},
                "f": {"0": 1}
            }
        });
        let report = analyze(
            &[first, second],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            false,
        )
        .unwrap();
        assert_eq!(
            report
                .rows
                .iter()
                .find(|row| row.id == first_unit.id)
                .unwrap()
                .coverage,
            Coverage::measured(0, 1).unwrap()
        );
        assert_eq!(
            report
                .rows
                .iter()
                .find(|row| row.id == second_unit.id)
                .unwrap()
                .coverage,
            Coverage::measured(1, 1).unwrap()
        );
    }

    #[test]
    fn unmatched_entries_are_structured_in_json_and_text_reports() {
        let path = ProjectRelativePath::new("diagnostics.ts").unwrap();
        let source = "function known() { return 1; }\n";
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let unit = super::source::analyze_source(&path, source)
            .unwrap()
            .pop()
            .unwrap();
        let coverage = json!({
            "diagnostics.ts": {
                "fnMap": {
                    "0": {"name": "stale", "loc": {"start": {"line": 1, "column": 0}, "end": {"line": 1, "column": 1}}}
                },
                "f": {"0": 0}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            true,
        )
        .unwrap();
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.category == DiagnosticCategory::CoverageAttribution
                && diagnostic
                    .message
                    .contains("unmatched Istanbul function-map entry")
        }));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.category == DiagnosticCategory::MissingEvidence
                && diagnostic.message.contains(&unit.id)
        }));
        let text = render_text(&report);
        assert!(text.contains("diagnostic [coverage_attribution]"));
        let json: serde_json::Value = serde_json::from_str(&render_json(&report).unwrap()).unwrap();
        assert!(json["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| { diagnostic["category"] == "coverage_attribution" }));
    }

    #[test]
    fn null_istanbul_end_columns_normalize_to_line_end_without_invalid_offsets() {
        let source = "function nullable() { return 1; }\n";
        let path = ProjectRelativePath::new("nullable.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let coverage = json!({
            "nullable.ts": {
                "fnMap": {
                    "0": {"name": "nullable", "loc": {
                        "start": {"line": 1, "column": 20},
                        "end": {"line": 1, "column": null}
                    }}
                },
                "statementMap": {
                    "0": {"start": {"line": 1, "column": 22}, "end": {"line": 1, "column": null}}
                },
                "f": {"0": 1},
                "s": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            false,
        )
        .unwrap();
        let row = report
            .rows
            .iter()
            .find(|row| row.name == "nullable")
            .unwrap();
        assert_eq!(row.coverage, Coverage::measured(1, 1).unwrap());
        assert!(row.range.start.offset < row.range.end.offset);
    }

    #[test]
    fn crossing_sibling_ranges_remain_unmatched_instead_of_shortest_guessing() {
        let source = "function first() { return 1; } function second() { return 2; }\n";
        let path = ProjectRelativePath::new("siblings.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let coverage = json!({
            "siblings.ts": {
                "fnMap": {
                    "0": {"name": "first", "loc": {
                        "start": {"line": 1, "column": 21},
                        "end": {"line": 1, "column": 53}
                    }}
                },
                "statementMap": {
                    "0": {"start": {"line": 1, "column": 21}, "end": {"line": 1, "column": 53}}
                },
                "f": {"0": 1},
                "s": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            true,
        )
        .unwrap();
        assert!(report
            .rows
            .iter()
            .all(|row| matches!(row.coverage, Coverage::Unknown { .. })));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.category == DiagnosticCategory::CoverageAttribution
                && diagnostic
                    .message
                    .contains("unmatched Istanbul function-map entry")
        }));
    }

    #[test]
    fn anonymous_istanbul_function_names_match_unique_arrow_locations() {
        let source = "const callback = () => 1;\n";
        let path = ProjectRelativePath::new("anonymous.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let unit = super::source::analyze_source(&path, source)
            .unwrap()
            .pop()
            .unwrap();
        let coverage = json!({
            "anonymous.ts": {
                "fnMap": {
                    "0": {"name": "(anonymous_0)", "loc": {
                        "start": {"line": 1, "column": 16},
                        "end": {"line": 1, "column": 24}
                    }}
                },
                "f": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            false,
        )
        .unwrap();
        let row = report.rows.iter().find(|row| row.id == unit.id).unwrap();
        assert_eq!(row.coverage, Coverage::measured(1, 1).unwrap());
    }

    #[test]
    fn nested_anonymous_vitest_envelopes_use_decl_to_select_the_child() {
        let source = "const smellCounter = (values: number[]) => {\n  return values.map((value) => {\n    return value + 1;\n  });\n};\n";
        let path = ProjectRelativePath::new("smell-counter.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let units = super::source::analyze_source(&path, source).unwrap();
        assert_eq!(units.len(), 2);
        let parent = units
            .iter()
            .find(|unit| unit.name == "smellCounter")
            .unwrap();
        let child = units.iter().find(|unit| unit.id != parent.id).unwrap();
        let broad_end = |line| json!({ "line": line, "column": null });
        let coverage = json!({
            "smell-counter.ts": {
                "fnMap": {
                    "0": {
                        "name": "(anonymous_0)",
                        "loc": {"start": istanbul_position(source, parent.range.start), "end": broad_end(5)},
                        "decl": istanbul_range(source, parent.range)
                    },
                    "1": {
                        "name": "(anonymous_1)",
                        "loc": {"start": istanbul_position(source, child.range.start), "end": broad_end(4)},
                        "decl": istanbul_range(source, child.range)
                    }
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
        let parent_row = report.rows.iter().find(|row| row.id == parent.id).unwrap();
        let child_row = report.rows.iter().find(|row| row.id == child.id).unwrap();
        assert_eq!(parent_row.coverage, Coverage::measured(0, 1).unwrap());
        assert_eq!(child_row.coverage, Coverage::measured(1, 1).unwrap());
    }

    #[test]
    fn nested_anonymous_envelope_without_decl_fails_closed() {
        let source = "const smellCounter = (values: number[]) => {\n  return values.map((value) => {\n    return value + 1;\n  });\n};\n";
        let path = ProjectRelativePath::new("smell-counter-ambiguous.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let units = super::source::analyze_source(&path, source).unwrap();
        let child = units
            .iter()
            .max_by_key(|unit| unit.range.start.offset)
            .unwrap();
        let coverage = json!({
            "smell-counter-ambiguous.ts": {
                "fnMap": {
                    "0": {
                        "name": "(anonymous_1)",
                        "loc": {"start": istanbul_position(source, child.range.start), "end": {"line": 4, "column": null}}
                    }
                },
                "f": {"0": 1}
            }
        });
        let error = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            false,
        )
        .unwrap_err();
        assert!(
            matches!(error, CoreError::CoverageAttribution(message) if message.contains("anonymous"))
        );
    }

    #[test]
    fn anonymous_constructor_decl_prefix_beats_nested_arrow_candidate() {
        let source = "class Service {\n  constructor() {\n    const nested = () => 1;\n    this.value = nested;\n  }\n}\n";
        let path = ProjectRelativePath::new("constructor.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let units = super::source::analyze_source(&path, source).unwrap();
        let constructor = units
            .iter()
            .find(|unit| unit.kind == FunctionKind::Constructor)
            .unwrap();
        let nested = units.iter().find(|unit| unit.id != constructor.id).unwrap();
        let coverage = json!({
            "constructor.ts": {
                "fnMap": {
                    "0": {
                        "name": "(anonymous_0)",
                        "loc": {"start": istanbul_position(source, nested.range.start), "end": {"line": 3, "column": null}},
                        "decl": {"start": position(2, 0), "end": position(2, 13)}
                    }
                },
                "f": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            true,
        )
        .unwrap();
        let constructor_row = report
            .rows
            .iter()
            .find(|row| row.id == constructor.id)
            .unwrap();
        let nested_row = report.rows.iter().find(|row| row.id == nested.id).unwrap();
        assert_eq!(constructor_row.coverage, Coverage::measured(1, 1).unwrap());
        assert!(matches!(nested_row.coverage, Coverage::Unknown { .. }));
    }

    #[test]
    fn real_short_circuit_function_fixture_keeps_statement_coverage_local() {
        let source = "function isShortCircuit(value: boolean) { return value && value; }\n";
        let path = ProjectRelativePath::new("is-short-circuit.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let unit = super::source::analyze_source(&path, source)
            .unwrap()
            .pop()
            .unwrap();
        let coverage = json!({
            "is-short-circuit.ts": {
                "fnMap": {"0": {"name": "isShortCircuit", "loc": range(unit.body_range)}},
                "statementMap": {"0": range(unit.body_range)},
                "f": {"0": 1},
                "s": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            false,
        )
        .unwrap();
        let row = report.rows.iter().find(|row| row.id == unit.id).unwrap();
        assert_eq!(row.complexity.get(), 2);
        assert_eq!(row.coverage, Coverage::measured(1, 1).unwrap());
    }

    #[test]
    fn statement_ownership_requires_the_complete_range_inside_a_body() {
        let source = "function only() {\n  return 1;\n}\nconst outside = 0;\n";
        let path = ProjectRelativePath::new("statement-range.ts").unwrap();
        let source_file = SourceFile {
            path: path.clone(),
            source: source.to_string(),
        };
        let unit = super::source::analyze_source(&path, source)
            .unwrap()
            .pop()
            .unwrap();
        let coverage = json!({
            "statement-range.ts": {
                "fnMap": {"0": {"name": "only", "loc": range(unit.body_range)}},
                "statementMap": {"0": {
                    "start": {"line": 2, "column": 2},
                    "end": {"line": 4, "column": 1}
                }},
                "s": {"0": 1}
            }
        });
        let report = analyze(
            &[source_file],
            &serde_json::to_string(&coverage).unwrap(),
            8,
            true,
        )
        .unwrap();
        let row = report.rows.iter().find(|row| row.id == unit.id).unwrap();
        assert!(matches!(row.coverage, Coverage::Unknown { .. }));
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.category == DiagnosticCategory::CoverageAttribution
                && diagnostic.message.contains("no compatible source function")
        }));
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
