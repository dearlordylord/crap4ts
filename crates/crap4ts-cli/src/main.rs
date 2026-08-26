use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use clap::{error::ErrorKind, Parser, ValueEnum};
use crap4ts_core::{analyze_with_root, collect_sources, render_json, render_text};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Parser)]
#[command(
    name = "crap4ts",
    version,
    about = "Calculate CRAP complexity risk for TypeScript functions",
    long_about = "Analyze TypeScript/TSX source against an existing Istanbul JSON artifact."
)]
struct Cli {
    /// Existing Istanbul coverage-final.json (generated mode is not part of v1 minimal path).
    #[arg(
        short = 'c',
        long = "coverage",
        visible_alias = "coverage-file",
        value_name = "PATH"
    )]
    coverage: PathBuf,

    /// Render a human-readable report or the versioned JSON report.
    #[arg(short = 'f', long = "format", value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Optional shorthand for --format json.
    #[arg(long, conflicts_with = "format")]
    json: bool,

    /// Maximum permitted CRAP score. A score strictly above this value fails with status 2.
    #[arg(
        short = 't',
        long = "threshold",
        default_value_t = 8,
        value_name = "NUMBER"
    )]
    threshold: u32,

    /// Keep rows whose coverage cannot be measured, assigning them unknown rather than failing.
    #[arg(long = "report-only")]
    report_only: bool,

    /// Project root used to make source identities deterministic and constrain discovery.
    #[arg(long = "project-root", default_value = ".", value_name = "PATH")]
    project_root: PathBuf,

    /// Explicit source roots or files. With no value, the project root is scanned.
    #[arg(value_name = "SOURCE")]
    source_paths: Vec<PathBuf>,

    /// Additional source root or file (repeatable). `--source-root` is an alias.
    #[arg(long = "source", visible_alias = "source-root", value_name = "PATH")]
    source_options: Vec<PathBuf>,
}

fn main() {
    process::exit(run());
}

fn run() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let status = match error.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
                _ => 1,
            };
            let _ = error.print();
            return status;
        }
    };

    match execute(&cli) {
        Ok(status) => status,
        Err(message) => {
            render_failure(&message, cli.json || cli.format == OutputFormat::Json);
            1
        }
    }
}

fn render_failure(message: &str, json_output: bool) {
    let category = diagnostic_category(message);
    if json_output {
        let document = serde_json::json!({
            "diagnostics": [{"category": category, "message": message}]
        });
        // This is intentionally written to stderr: JSON stdout remains
        // reserved for completed versioned reports.
        eprintln!("{}", document);
    } else {
        eprintln!("error: {message}");
        eprintln!("diagnostic [{category}]: {message}");
    }
}

fn diagnostic_category(message: &str) -> &'static str {
    if message.starts_with("coverage attribution failed")
        || message.starts_with("coverage artifact contains ambiguous")
    {
        "coverage_attribution"
    } else if message.starts_with("coverage parsing failed") {
        "coverage_parsing"
    } else if message.starts_with("missing coverage evidence") {
        "missing_evidence"
    } else if message.starts_with("source parsing failed") {
        "source_parsing"
    } else {
        "configuration"
    }
}

fn execute(cli: &Cli) -> Result<i32, String> {
    let root = canonicalize_path(&cli.project_root, "project root")?;
    let mut requested = cli.source_paths.clone();
    requested.extend(cli.source_options.iter().cloned());
    let sources = collect_sources(&root, &requested).map_err(|error| error.to_string())?;
    if sources.is_empty() {
        return Err("configuration: source selection produced no TypeScript files".to_string());
    }

    let coverage_path = if cli.coverage.is_absolute() {
        cli.coverage.clone()
    } else {
        root.join(&cli.coverage)
    };
    let coverage = fs::read_to_string(&coverage_path).map_err(|error| {
        format!(
            "configuration: unable to read coverage artifact '{}': {error}",
            coverage_path.display()
        )
    })?;
    let report = analyze_with_root(&sources, &coverage, &root, cli.threshold, cli.report_only)
        .map_err(|error| error.to_string())?;

    if cli.json || cli.format == OutputFormat::Json {
        let document = render_json(&report)
            .map_err(|error| format!("configuration: unable to render JSON report: {error}"))?;
        println!("{document}");
    } else {
        print!("{}", render_text(&report));
    }

    if report.gate_breached() {
        eprintln!(
            "quality gate breached: one or more CRAP scores exceed {}",
            cli.threshold
        );
        Ok(2)
    } else {
        Ok(0)
    }
}

fn canonicalize_path(path: &Path, description: &str) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| {
        format!(
            "configuration: unable to resolve {description} '{}': {error}",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn clap_command_exposes_stable_name() {
        assert_eq!(Cli::command().get_name(), "crap4ts");
    }
}
