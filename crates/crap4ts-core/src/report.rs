//! Stable report renderers.

use crate::domain::{Coverage, Report};

/// Render the versioned JSON report. Keeping serialization behind the report
/// boundary lets another output format be added without changing analysis or
/// gate policy.
pub fn render_json(report: &Report) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

/// Render a report in the deterministic human-readable text format.
pub fn render_text(report: &Report) -> String {
    let mut output = String::new();
    if let Some(threshold) = report.threshold {
        output.push_str(&format!(
            "crap4ts report v{} (threshold: {threshold})\n",
            report.version
        ));
    } else {
        output.push_str(&format!(
            "crap4ts report v{} (package groups: {})\n",
            report.version,
            report.groups.len()
        ));
    }
    for group in &report.groups {
        output.push_str(&format!(
            "group {} root={} threshold={} report_only={}\n",
            group.name, group.root, group.threshold, group.report_only
        ));
    }
    for row in &report.rows {
        let range = format!(
            "{}:{}-{}:{}",
            row.range.start.line, row.range.start.column, row.range.end.line, row.range.end.column
        );
        let coverage = match &row.coverage {
            Coverage::Measured { fraction, .. } => format!("{:.2}%", fraction * 100.0),
            Coverage::Unknown { reason } => format!("unknown ({reason})"),
        };
        let score = row
            .crap
            .map_or_else(|| "unknown".to_string(), |score| format!("{score:.6}"));
        let group = row
            .group
            .as_deref()
            .map_or_else(String::new, |group| format!("[{group}] "));
        output.push_str(&format!(
            "{}{} {} [{}] {} complexity={} coverage={} crap={}\n",
            group,
            row.path,
            range,
            row.kind.as_label(),
            row.name,
            row.complexity.get(),
            coverage,
            score
        ));
    }
    if report.rows.is_empty() {
        output.push_str("(no executable TypeScript functions)\n");
    }
    for diagnostic in &report.diagnostics {
        if let Some(group) = &diagnostic.group {
            output.push_str(&format!(
                "diagnostic [{}] group={group}: {}\n",
                diagnostic.category.as_label(),
                diagnostic.message
            ));
        } else {
            output.push_str(&format!(
                "diagnostic [{}]: {}\n",
                diagnostic.category.as_label(),
                diagnostic.message
            ));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{GroupName, GroupRoot, ReportGroup};

    #[test]
    fn aggregate_text_does_not_label_a_global_threshold() {
        let report = Report {
            version: crate::domain::AGGREGATE_REPORT_VERSION,
            threshold: None,
            rows: Vec::new(),
            diagnostics: Vec::new(),
            groups: vec![ReportGroup {
                name: GroupName::new("core").unwrap(),
                root: GroupRoot::new("packages/core").unwrap(),
                threshold: 8,
                report_only: false,
                threshold_overrides: std::collections::BTreeMap::new(),
            }],
        };
        let text = render_text(&report);
        assert!(text.starts_with("crap4ts report v2 (package groups: 1)\n"));
        assert!(!text.contains("(threshold:"));
        assert!(text.contains("group core root=packages/core threshold=8 report_only=false"));
    }
}
