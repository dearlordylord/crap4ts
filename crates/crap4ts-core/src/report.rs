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
    output.push_str(&format!(
        "crap4ts report v{} (threshold: {})\n",
        report.version, report.threshold
    ));
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
        output.push_str(&format!(
            "{} {} [{}] {} complexity={} coverage={} crap={}\n",
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
    output
}
