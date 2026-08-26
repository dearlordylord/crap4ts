mod config;
mod generation;

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process,
};

use clap::{error::ErrorKind, ArgAction, Parser};
use config::{ConfigValues, OutputFormat, DEFAULT_THRESHOLD};
use crap4ts_core::{
    aggregate_reports, analyze_with_adapter_and_policy, collect_sources_with_options,
    make_coverage_adapter, render_json, render_text, validate_sources, CoverageAdapter,
    CoverageFormat, Diagnostic, DiagnosticCategory, GroupName, GroupRoot, PackageReport,
    ProjectRelativePath, SourceFile, ThresholdPolicy,
};

#[derive(Debug, Parser)]
#[command(
    name = "crap4ts",
    version,
    about = "Calculate CRAP complexity risk for TypeScript functions",
    long_about = "Analyze TypeScript/TSX source against an existing or freshly generated Istanbul JSON or LCOV artifact."
)]
struct Cli {
    /// Optional project-local JSON configuration file. If omitted, crap4ts.json
    /// (or a supported dot/config spelling) is discovered at the project root.
    #[arg(long = "config", visible_alias = "config-file", value_name = "PATH")]
    config: Option<PathBuf>,

    /// Coverage artifact path. Generated mode removes and recreates this path;
    /// configuration may provide it.
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

    /// Program to run to generate fresh coverage. Repeat --coverage-arg for
    /// arguments; the command is executed directly without a shell.
    #[arg(
        long = "coverage-command",
        visible_alias = "command",
        value_name = "PROGRAM",
        conflicts_with = "no_generate"
    )]
    coverage_command: Option<OsString>,

    /// One argument for --coverage-command (repeatable, preserves boundaries).
    #[arg(
        long = "coverage-arg",
        value_name = "ARG",
        action = ArgAction::Append,
        allow_hyphen_values = true,
        conflicts_with = "no_generate"
    )]
    coverage_args: Vec<OsString>,

    /// Require a configured coverage command to run before analysis.
    #[arg(long = "generate", conflicts_with = "no_generate")]
    generate: bool,

    /// Use an existing artifact even when configuration contains a command.
    #[arg(long = "no-generate", conflicts_with = "generate")]
    no_generate: bool,

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
                if let Some(group) = &diagnostic.group {
                    eprintln!(
                        "diagnostic [{}] group={group}: {}",
                        diagnostic.category.as_label(),
                        diagnostic.message
                    );
                } else {
                    eprintln!(
                        "diagnostic [{}]: {}",
                        diagnostic.category.as_label(),
                        diagnostic.message
                    );
                }
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
    } else if message.starts_with("coverage command") {
        "coverage_command"
    } else if message.starts_with("unsafe coverage artifact path") {
        "unsafe_path"
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

    if values.groups.is_some() {
        return execute_groups(cli, &root, &values);
    }

    execute_single(cli, &root, &values)
}

fn execute_single(cli: &Cli, root: &Path, values: &ConfigValues) -> Result<i32, CliFailure> {
    let coverage = cli
        .coverage
        .clone()
        .or_else(|| values.coverage.clone())
        .ok_or_else(|| {
            "configuration: coverage artifact is required (provide --coverage or config)"
                .to_string()
        })?;
    let command = resolved_coverage_command(cli, values)?;
    let mut requested = if cli.source_paths.is_empty() && cli.source_options.is_empty() {
        values.sources.clone().unwrap_or_default()
    } else {
        cli.source_paths.clone()
    };
    if !(cli.source_paths.is_empty() && cli.source_options.is_empty()) {
        requested.extend(cli.source_options.iter().cloned());
    }
    let sources = collect_sources_with_options(root, &requested, values.source_selection)
        .map_err(|error| error.to_string())?;
    if sources.is_empty() {
        return Err(
            "configuration: source selection produced no TypeScript files"
                .to_string()
                .into(),
        );
    }

    let format = resolved_format(cli, values);
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
    let policy = config::policy(values, threshold);

    let coverage = if let Some(command) = command {
        generation::generate(root, &coverage, &command).map_err(CliFailure::from)?
    } else {
        let coverage_path = if coverage.is_absolute() {
            coverage
        } else {
            root.join(coverage)
        };
        fs::read_to_string(&coverage_path).map_err(|error| {
            format!(
                "configuration: unable to read coverage artifact '{}': {error}",
                coverage_path.display()
            )
        })?
    };
    let adapter =
        make_coverage_adapter(coverage_format, &coverage, root).map_err(|error| CliFailure {
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

/// A fully resolved, preflighted group. No coverage command is run until
/// every group has an instance of this atomic bundle, so
/// configuration/source/path failures are observed before any generated
/// artifact is removed.
struct ResolvedPackageGroup {
    name: GroupName,
    root: PathBuf,
    root_identity: GroupRoot,
    sources: Vec<SourceFile>,
    artifact: PathBuf,
    coverage_format: CoverageFormat,
    command: Option<Vec<String>>,
    policy: ThresholdPolicy,
    report_only: bool,
    /// Existing artifacts are parsed during preflight and retained in memory.
    /// Generated groups fill this field only after their command succeeds.
    adapter: Option<Box<dyn CoverageAdapter>>,
}

fn execute_groups(cli: &Cli, root: &Path, values: &ConfigValues) -> Result<i32, CliFailure> {
    reject_multi_group_cli_overrides(cli)?;
    let groups = values
        .groups
        .as_ref()
        .expect("group branch checked by execute");

    let mut resolved = Vec::with_capacity(groups.len());
    let mut selected_files = BTreeMap::<String, String>::new();
    let mut artifacts = BTreeMap::<PathBuf, String>::new();

    // This is the complete preflight pass. It performs only validation and
    // reads source files; generation side effects start after this loop.
    for group in groups {
        let group_name = GroupName::new(&group.name)
            .map_err(|error| group_failure(&group.name, error.to_string(), &[]))?;
        let group_root = resolve_group_root(root, group.root.as_deref())
            .map_err(|error| group_failure(&group.name, error, &[]))?;
        let root_identity_text = repository_identity(root, &group_root)
            .map_err(|error| group_failure(&group.name, error, &[]))?;
        let root_identity = GroupRoot::new(&root_identity_text)
            .map_err(|error| group_failure(&group.name, error.to_string(), &[]))?;
        let requested = group
            .settings
            .sources
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|path| group_relative_path(path, root_identity.as_str()))
            .collect::<Vec<_>>();
        let sources =
            collect_sources_with_options(&group_root, &requested, group.settings.source_selection)
                .map_err(|error| group_failure(&group.name, error.to_string(), &[]))?;
        if sources.is_empty() {
            return Err(group_failure(
                &group.name,
                "configuration: source selection produced no TypeScript files".to_string(),
                &[],
            ));
        }
        validate_sources(&sources)
            .map_err(|error| group_failure(&group.name, error.to_string(), &[]))?;

        for source in &sources {
            let absolute = group_root.join(source.path.as_str());
            let canonical = fs::canonicalize(&absolute).map_err(|error| {
                group_failure(
                    &group.name,
                    format!(
                        "configuration: unable to resolve selected source '{}': {error}",
                        source.path
                    ),
                    &[],
                )
            })?;
            let identity = ProjectRelativePath::from_filesystem_path(&canonical, root)
                .map_err(|error| group_failure(&group.name, error.to_string(), &[]))?
                .to_string();
            if let Some(previous) = selected_files.insert(identity.clone(), group.name.clone()) {
                return Err(group_failure(
                    &group.name,
                    format!(
                        "configuration: selected source '{}' overlaps package group '{}'",
                        identity, previous
                    ),
                    &[],
                ));
            }
        }

        let configured_artifact = group
            .settings
            .coverage
            .clone()
            .map(|path| group_relative_path(path, root_identity.as_str()))
            .ok_or_else(|| {
                group_failure(
                    &group.name,
                    "configuration: coverage artifact is required for every package group"
                        .to_string(),
                    &[],
                )
            })?;
        let artifact = generation::validate_artifact_path(&group_root, &configured_artifact)
            .map_err(|error| group_failure(&group.name, error, &[]))?;
        let artifact_key =
            artifact_identity(&artifact).map_err(|error| group_failure(&group.name, error, &[]))?;
        if let Some(previous) = artifacts.insert(artifact_key, group.name.clone()) {
            return Err(group_failure(
                &group.name,
                format!(
                    "configuration: coverage artifact is shared with package group '{}'",
                    previous
                ),
                &[],
            ));
        }

        let command = group.settings.coverage_command.clone();
        if let Some(command) = &command {
            generation::validate_command(command)
                .map_err(|error| group_failure(&group.name, error, &[]))?;
        }
        let threshold = group.settings.threshold.unwrap_or(DEFAULT_THRESHOLD);
        let policy = group_policy(&group.settings, threshold, root_identity.as_str())
            .map_err(|error| group_failure(&group.name, error, &[]))?;
        let adapter = if command.is_none() {
            let coverage = read_group_artifact(&group_root, &artifact)
                .map_err(|error| group_failure(&group.name, error, &[]))?;
            let adapter = make_coverage_adapter(
                group.settings.coverage_format.unwrap_or_default(),
                &coverage,
                &group_root,
            )
            .map_err(|error| {
                group_failure(
                    &group.name,
                    error.to_string(),
                    &qualified_diagnostics(&group.name, error.diagnostics()),
                )
            })?;
            Some(adapter)
        } else {
            None
        };
        resolved.push(ResolvedPackageGroup {
            name: group_name,
            root: group_root,
            root_identity,
            sources,
            artifact,
            coverage_format: group.settings.coverage_format.unwrap_or_default(),
            command,
            policy,
            report_only: group.settings.report_only.unwrap_or(false),
            adapter,
        });
    }

    let mut packages = Vec::with_capacity(resolved.len());
    let mut breached = false;
    for mut group in resolved {
        let adapter = if let Some(adapter) = group.adapter.take() {
            adapter
        } else {
            let coverage = acquire_group_coverage(&group)
                .map_err(|error| group_failure(group.name.as_str(), error, &[]))?;
            make_coverage_adapter(group.coverage_format, &coverage, &group.root).map_err(
                |error| {
                    group_failure(
                        group.name.as_str(),
                        error.to_string(),
                        &qualified_diagnostics(group.name.as_str(), error.diagnostics()),
                    )
                },
            )?
        };
        let report = analyze_with_adapter_and_policy(
            &group.sources,
            adapter.as_ref(),
            &group.policy,
            group.report_only,
        )
        .map_err(|error| {
            group_failure(
                group.name.as_str(),
                error.to_string(),
                &qualified_diagnostics(group.name.as_str(), error.diagnostics()),
            )
        })?;
        breached |= group.policy.gate_breached(&report);
        packages.push(PackageReport {
            name: group.name,
            root: group.root_identity,
            policy: group.policy,
            report_only: group.report_only,
            report,
        });
    }

    // Rendering occurs exactly once and only after every package has reached
    // a completed in-memory report.
    let report = aggregate_reports(packages).map_err(|error| CliFailure {
        message: error.to_string(),
        diagnostics: error.diagnostics().to_vec(),
    })?;
    let format = resolved_format(cli, values);
    if format == OutputFormat::Json {
        let document = render_json(&report)
            .map_err(|error| format!("configuration: unable to render JSON report: {error}"))?;
        println!("{document}");
    } else {
        print!("{}", render_text(&report));
    }
    if breached {
        eprintln!("quality gate breached: one or more CRAP scores exceed the effective threshold");
        Ok(2)
    } else {
        Ok(0)
    }
}

fn reject_multi_group_cli_overrides(cli: &Cli) -> Result<(), CliFailure> {
    let has_sources = !cli.source_paths.is_empty() || !cli.source_options.is_empty();
    if cli.coverage.is_some()
        || cli.coverage_format.is_some()
        || cli.coverage_command.is_some()
        || !cli.coverage_args.is_empty()
        || cli.generate
        || cli.no_generate
        || cli.threshold.is_some()
        || cli.report_only
        || cli.no_report_only
        || has_sources
    {
        return Err(
            "configuration: single-project analysis flags cannot be used with groups; set the value on each named group"
                .to_string()
                .into(),
        );
    }
    Ok(())
}

fn resolve_group_root(repo_root: &Path, configured: Option<&Path>) -> Result<PathBuf, String> {
    let configured = configured.unwrap_or_else(|| Path::new("."));
    let text = configured.to_str().ok_or_else(|| {
        format!(
            "configuration: package group root '{}' must be valid UTF-8",
            configured.display()
        )
    })?;
    if text.is_empty() || text.contains('\0') {
        return Err(format!(
            "configuration: package group root '{}' is empty or contains NUL",
            configured.display()
        ));
    }
    let normalized = text.replace('\\', "/");
    if !configured.is_absolute() && normalized.split('/').any(|component| component == "..") {
        return Err(format!(
            "configuration: package group root '{}' contains parent traversal",
            configured.display()
        ));
    }
    let path = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        repo_root.join(normalized)
    };
    let canonical = fs::canonicalize(&path).map_err(|error| {
        format!(
            "configuration: unable to resolve package group root '{}': {error}",
            path.display()
        )
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        format!(
            "configuration: unable to inspect package group root '{}': {error}",
            canonical.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "configuration: package group root '{}' is not a directory",
            path.display()
        ));
    }
    if !canonical.starts_with(repo_root) {
        return Err(format!(
            "configuration: package group root '{}' escapes repository root '{}'",
            path.display(),
            repo_root.display()
        ));
    }
    Ok(canonical)
}

fn repository_identity(repo_root: &Path, path: &Path) -> Result<String, String> {
    let relative = path.strip_prefix(repo_root).map_err(|_| {
        format!(
            "configuration: package group root '{}' escapes repository root '{}'",
            path.display(),
            repo_root.display()
        )
    })?;
    let value = relative
        .to_str()
        .ok_or_else(|| {
            format!(
                "configuration: package group root '{}' is not UTF-8",
                path.display()
            )
        })?
        .replace('\\', "/");
    Ok(if value.is_empty() {
        ".".to_string()
    } else {
        value
    })
}

/// Accept both the documented group-local spelling and a repository-root
/// spelling for paths copied from a monorepo manifest. The latter is reduced
/// to the local identity before source selection/coverage parsing, so a group
/// can never accidentally inspect a sibling package.
fn group_relative_path(path: PathBuf, root_identity: &str) -> PathBuf {
    if path.is_absolute() || root_identity == "." {
        return path;
    }
    let Some(value) = path.to_str() else {
        return path;
    };
    let normalized = value.replace('\\', "/");
    let prefix = format!("{root_identity}/");
    if let Some(local) = normalized.strip_prefix(&prefix) {
        PathBuf::from(local)
    } else {
        path
    }
}

fn group_policy(
    values: &ConfigValues,
    global: u32,
    root_identity: &str,
) -> Result<ThresholdPolicy, String> {
    values.threshold_overrides.iter().try_fold(
        ThresholdPolicy::new(global),
        |policy, (path, threshold)| {
            let path = group_relative_path(PathBuf::from(path.as_str()), root_identity);
            let path =
                ProjectRelativePath::new(path.to_string_lossy().as_ref()).map_err(|error| {
                    format!(
                        "configuration: invalid threshold override path '{}': {error}",
                        path.display()
                    )
                })?;
            Ok(policy.with_path_override(path, *threshold))
        },
    )
}

fn artifact_identity(path: &Path) -> Result<PathBuf, String> {
    // Generated targets may have several missing parent directories. Walk to
    // the nearest existing ancestor, canonicalize only that safe portion,
    // then append the validated missing components in their original order.
    // `validate_artifact_path` has already rejected traversal and symlinked
    // ancestors, so this preserves aliases while still catching duplicate
    // targets that use different lexical spellings.
    let mut current = path.to_path_buf();
    let mut missing = Vec::<OsString>::new();
    loop {
        match fs::symlink_metadata(&current) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let component = current.file_name().ok_or_else(|| {
                    format!(
                        "configuration: coverage artifact '{}' has no file name",
                        path.display()
                    )
                })?;
                missing.push(component.to_os_string());
                current = current
                    .parent()
                    .ok_or_else(|| {
                        format!(
                            "configuration: coverage artifact '{}' has no existing ancestor",
                            path.display()
                        )
                    })?
                    .to_path_buf();
            }
            Err(error) => {
                return Err(format!(
                    "configuration: unable to inspect coverage artifact '{}': {error}",
                    current.display()
                ));
            }
        }
    }

    let mut identity = fs::canonicalize(&current).map_err(|error| {
        format!(
            "configuration: unable to resolve coverage artifact ancestor '{}': {error}",
            current.display()
        )
    })?;
    for component in missing.iter().rev() {
        identity.push(component);
    }
    Ok(identity)
}

fn read_group_artifact(root: &Path, configured: &Path) -> Result<String, String> {
    // Recheck the validated path immediately before reading it. This keeps an
    // existing-mode symlink replacement from escaping the group boundary and
    // mirrors generated-mode safety.
    let artifact = generation::validate_artifact_path(root, configured)?;
    fs::read_to_string(&artifact).map_err(|error| {
        format!(
            "configuration: unable to read coverage artifact '{}': {error}",
            artifact.display()
        )
    })
}

fn acquire_group_coverage(group: &ResolvedPackageGroup) -> Result<String, String> {
    if let Some(command) = &group.command {
        generation::generate(&group.root, &group.artifact, command)
    } else {
        read_group_artifact(&group.root, &group.artifact)
    }
}

fn qualified_diagnostics(group: &str, diagnostics: &[Diagnostic]) -> Vec<Diagnostic> {
    diagnostics
        .iter()
        .cloned()
        .map(|diagnostic| diagnostic.with_group(group.to_string()))
        .collect()
}

fn group_failure(group: &str, message: String, diagnostics: &[Diagnostic]) -> CliFailure {
    let diagnostics = if diagnostics.is_empty() {
        vec![Diagnostic {
            group: Some(group.to_string()),
            category: category_for_message(&message),
            message: message.clone(),
        }]
    } else {
        diagnostics.to_vec()
    };
    CliFailure {
        message: format!("package group '{group}': {message}"),
        diagnostics,
    }
}

fn category_for_message(message: &str) -> DiagnosticCategory {
    match diagnostic_category(message) {
        "coverage_attribution" => DiagnosticCategory::CoverageAttribution,
        "coverage_parsing" => DiagnosticCategory::CoverageParsing,
        "missing_evidence" => DiagnosticCategory::MissingEvidence,
        "source_parsing" => DiagnosticCategory::SourceParsing,
        "coverage_command" => DiagnosticCategory::CoverageCommand,
        "unsafe_path" => DiagnosticCategory::UnsafePath,
        _ => DiagnosticCategory::Configuration,
    }
}

fn resolved_coverage_command(
    cli: &Cli,
    values: &ConfigValues,
) -> Result<Option<Vec<String>>, CliFailure> {
    if cli.coverage_command.is_none() && !cli.coverage_args.is_empty() {
        return Err("configuration: --coverage-arg requires --coverage-command"
            .to_string()
            .into());
    }
    if cli.no_generate {
        return Ok(None);
    }
    let command = if let Some(program) = &cli.coverage_command {
        let program = program.to_str().ok_or_else(|| {
            CliFailure::from("configuration: --coverage-command must be valid UTF-8".to_string())
        })?;
        let mut command = Vec::with_capacity(cli.coverage_args.len() + 1);
        command.push(program.to_string());
        command.extend(
            cli.coverage_args
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned()),
        );
        Some(command)
    } else {
        values.coverage_command.clone()
    };
    if cli.generate && command.is_none() {
        return Err(
            "configuration: --generate requires a configured coverage command"
                .to_string()
                .into(),
        );
    }
    Ok(command)
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
