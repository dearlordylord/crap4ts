mod config;

use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use clap::{error::ErrorKind, Parser};
use config::{ConfigValues, OutputFormat, DEFAULT_THRESHOLD};
use crap4ts_core::{
    analyze_with_adapter_and_policy, collect_sources, make_coverage_adapter, render_json,
    render_text, CoverageFormat, Diagnostic,
};

#[derive(Debug, Parser)]
#[command(
    name = "crap4ts",
    version,
    about = "Calculate CRAP complexity risk for TypeScript functions",
    long_about = "Analyze TypeScript/TSX source against an existing Istanbul JSON or LCOV artifact."
)]
struct Cli {
    /// Optional project-local JSON configuration file. If omitted, crap4ts.json
    /// (or a supported dot/config spelling) is discovered at the project root.
    #[arg(long = "config", visible_alias = "config-file", value_name = "PATH")]
    config: Option<PathBuf>,

    /// Existing coverage artifact. Configuration may provide this value.
    #[arg(
        short = 'c',
        long = "coverage",
        visible_alias = "coverage-file",
        value_name = "PATH"
    )]
    coverage: Option<PathBuf>,

    /// Coverage artifact format. Configuration may provide this value.
    #[arg(long = "coverage-format", value_name = "FORMAT")]
    coverage_format: Option<CoverageFormat>,

    /// Render a human-readable report or the versioned JSON report.
    #[arg(short = 'f', long = "format", value_name = "FORMAT")]
    format: Option<OutputFormat>,

    /// Optional shorthand for --format json.
    #[arg(long, conflicts_with = "format")]
    json: bool,

    /// Maximum permitted CRAP score. A score strictly above this value fails with status 2.
    #[arg(short = 't', long = "threshold", value_name = "NUMBER")]
    threshold: Option<u32>,

    /// Keep rows whose coverage cannot be measured, assigning them unknown rather than failing.
    #[arg(long = "report-only", conflicts_with = "no_report_only")]
    report_only: bool,

    /// Explicitly fail on missing evidence, overriding report-only configuration.
    #[arg(long = "no-report-only", conflicts_with = "report_only")]
    no_report_only: bool,

    /// Project root used to make source identities deterministic and constrain discovery.
    #[arg(long = "project-root", value_name = "PATH")]
    project_root: Option<PathBuf>,

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
        Err(failure) => {
            render_failure(
                &failure.message,
                cli.json || cli.format == Some(OutputFormat::Json),
                &failure.diagnostics,
            );
            1
        }
    }
}

#[derive(Debug)]
struct CliFailure {
    message: String,
    diagnostics: Vec<Diagnostic>,
}

impl From<String> for CliFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            diagnostics: Vec::new(),
        }
    }
}

fn render_failure(message: &str, json_output: bool, diagnostics: &[Diagnostic]) {
    let category = diagnostic_category(message);
    if json_output {
        let entries = if diagnostics.is_empty() {
            vec![serde_json::json!({"category": category, "message": message})]
        } else {
            diagnostics
                .iter()
                .map(|diagnostic| {
                    serde_json::to_value(diagnostic).expect("diagnostic is serializable")
                })
                .collect()
        };
        let document = serde_json::json!({
            "diagnostics": entries
        });
        // JSON stdout remains reserved for completed versioned reports.
        eprintln!("{}", document);
    } else {
        eprintln!("error: {message}");
        if diagnostics.is_empty() {
            eprintln!("diagnostic [{category}]: {message}");
        } else {
            for diagnostic in diagnostics {
                eprintln!(
                    "diagnostic [{}]: {}",
                    diagnostic.category.as_label(),
                    diagnostic.message
                );
            }
        }
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

fn execute(cli: &Cli) -> Result<i32, CliFailure> {
    let root_hint = cli
        .project_root
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    let root = canonicalize_path(root_hint, "project root")?;
    let values = load_config(cli, &root)?;

    let coverage = cli
        .coverage
        .clone()
        .or_else(|| values.coverage.clone())
        .ok_or_else(|| {
            "configuration: coverage artifact is required (provide --coverage or config)"
                .to_string()
        })?;
    let mut requested = if cli.source_paths.is_empty() && cli.source_options.is_empty() {
        values.sources.clone().unwrap_or_default()
    } else {
        cli.source_paths.clone()
    };
    if !(cli.source_paths.is_empty() && cli.source_options.is_empty()) {
        requested.extend(cli.source_options.iter().cloned());
    }
    let sources = collect_sources(&root, &requested).map_err(|error| error.to_string())?;
    if sources.is_empty() {
        return Err(
            "configuration: source selection produced no TypeScript files"
                .to_string()
                .into(),
        );
    }

    let format = resolved_format(cli, &values);
    let coverage_format = cli
        .coverage_format
        .or(values.coverage_format)
        .unwrap_or_default();
    let threshold = cli
        .threshold
        .or(values.threshold)
        .unwrap_or(DEFAULT_THRESHOLD);
    let report_only = if cli.report_only {
        true
    } else if cli.no_report_only {
        false
    } else {
        values.report_only.unwrap_or(false)
    };
    let policy = config::policy(&values, threshold);

    let coverage_path = if coverage.is_absolute() {
        coverage
    } else {
        root.join(coverage)
    };
    let coverage = fs::read_to_string(&coverage_path).map_err(|error| {
        format!(
            "configuration: unable to read coverage artifact '{}': {error}",
            coverage_path.display()
        )
    })?;
    let adapter =
        make_coverage_adapter(coverage_format, &coverage, &root).map_err(|error| CliFailure {
            message: error.to_string(),
            diagnostics: error.diagnostics().to_vec(),
        })?;
    let report = analyze_with_adapter_and_policy(&sources, adapter.as_ref(), &policy, report_only)
        .map_err(|error| CliFailure {
            message: error.to_string(),
            diagnostics: error.diagnostics().to_vec(),
        })?;

    if format == OutputFormat::Json {
        let document = render_json(&report)
            .map_err(|error| format!("configuration: unable to render JSON report: {error}"))?;
        println!("{document}");
    } else {
        print!("{}", render_text(&report));
    }

    if policy.gate_breached(&report) {
        eprintln!("quality gate breached: one or more CRAP scores exceed the effective threshold");
        Ok(2)
    } else {
        Ok(0)
    }
}

fn load_config(cli: &Cli, root: &Path) -> Result<ConfigValues, CliFailure> {
    let path = if let Some(path) = &cli.config {
        Some(resolve_config_argument(path)?)
    } else {
        config::discover(root)?
    };
    path.map_or_else(
        || Ok(ConfigValues::default()),
        |path| config::load(&path, root),
    )
    .map_err(Into::into)
}

fn resolve_config_argument(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let current = std::env::current_dir()
        .map_err(|error| format!("configuration: unable to resolve current directory: {error}"))?;
    Ok(current.join(path))
}

fn resolved_format(cli: &Cli, values: &ConfigValues) -> OutputFormat {
    if cli.json {
        OutputFormat::Json
    } else if let Some(format) = cli.format {
        format
    } else if values.json == Some(true) {
        OutputFormat::Json
    } else {
        values.format.unwrap_or_default()
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
