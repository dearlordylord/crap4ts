use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    process,
};

use clap::{error::ErrorKind, Parser, ValueEnum};
use crap4ts_core::{analyze, render_text, SourceFile};

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

    /// Additional source root or file (repeatable).
    #[arg(long = "source", value_name = "PATH")]
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
            eprintln!("error: {message}");
            1
        }
    }
}

fn execute(cli: &Cli) -> Result<i32, String> {
    let root = canonicalize_path(&cli.project_root, "project root")?;
    let mut requested = cli.source_paths.clone();
    requested.extend(cli.source_options.iter().cloned());
    if requested.is_empty() {
        requested.push(PathBuf::from("."));
    }
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
    let report = analyze(&sources, &coverage, cli.threshold, cli.report_only)
        .map_err(|error| error.to_string())?;

    if cli.json || cli.format == OutputFormat::Json {
        let document = serde_json::to_string_pretty(&report)
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

fn collect_sources(root: &Path, requested: &[PathBuf]) -> io::Result<Vec<SourceFile>> {
    let mut files = BTreeMap::new();
    for requested_path in requested {
        let absolute = if requested_path.is_absolute() {
            requested_path.clone()
        } else {
            root.join(requested_path)
        };
        let absolute = fs::canonicalize(absolute)?;
        if !absolute.starts_with(root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "configuration: source path '{}' escapes project root",
                    absolute.display()
                ),
            ));
        }
        collect_path(root, &absolute, &mut files)?;
    }
    files
        .into_iter()
        .map(|(path, source)| Ok(SourceFile { path, source }))
        .collect()
}

fn collect_path(root: &Path, path: &Path, files: &mut BTreeMap<String, String>) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        let target = fs::canonicalize(path)?;
        if !target.starts_with(root) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "configuration: symlink '{}' escapes project root",
                    path.display()
                ),
            ));
        }
        return collect_path(root, &target, files);
    }
    if metadata.is_dir() {
        let mut children = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            if child.file_type()?.is_dir() && excluded_directory(&child.file_name()) {
                continue;
            }
            collect_path(root, &child.path(), files)?;
        }
        return Ok(());
    }
    if !metadata.is_file() || !is_typescript(path) || is_declaration(path) {
        return Ok(());
    }
    let relative = path.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "source escaped project root",
        )
    })?;
    let identity = relative.to_string_lossy().replace('\\', "/");
    files.insert(identity, fs::read_to_string(path)?);
    Ok(())
}

fn is_typescript(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "tsx" | "mts" | "cts")
    )
}

fn is_declaration(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
        })
}

fn excluded_directory(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some("node_modules" | "target" | "dist" | "build" | "coverage" | ".git")
    )
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
