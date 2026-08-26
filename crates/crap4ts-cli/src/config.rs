//! Strict, data-only project configuration.
//!
//! Configuration is deliberately kept at the CLI boundary.  The file is
//! parsed as JSON into closed serde structs, so executable JavaScript (or an
//! accidental unknown option) cannot be loaded and ignored.  The resulting
//! values are merged with command-line options by `main`.

use std::{
    collections::{BTreeMap, HashSet},
    fmt, fs, io,
    path::{Path, PathBuf},
};

use clap::ValueEnum;
use crap4ts_core::{CoverageFormat, ProjectRelativePath, ThresholdPolicy};
use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer,
};

use crate::generation::CommandSpec;
pub(crate) const DEFAULT_CONFIG_FILE: &str = "crap4ts.json";
pub(crate) const DEFAULT_THRESHOLD: u32 = 8;

/// Deserialize an optional field while rejecting an explicit JSON `null`.
/// `#[serde(default)]` still supplies `None` when the field is absent.
fn reject_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)?
        .ok_or_else(|| de::Error::custom("null is not permitted; omit the field instead"))
        .map(Some)
}

struct DuplicateKeyVisitor;

impl<'de> Visitor<'de> for DuplicateKeyVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON data without duplicate object keys")
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_i64<E>(self, _: i64) -> Result<(), E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_u64<E>(self, _: u64) -> Result<(), E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_f64<E>(self, _: f64) -> Result<(), E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_str<E>(self, _: &str) -> Result<(), E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_none<E>(self) -> Result<(), E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E>
    where
        E: de::Error,
    {
        Ok(())
    }

    fn visit_map<A>(self, mut map: A) -> Result<(), A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(de::Error::custom(format!(
                    "duplicate JSON object key '{key}'"
                )));
            }
            map.next_value_seed(DuplicateKeySeed)?;
        }
        Ok(())
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<(), A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        while sequence.next_element_seed(DuplicateKeySeed)?.is_some() {}
        Ok(())
    }
}

struct DuplicateKeySeed;

impl<'de> de::DeserializeSeed<'de> for DuplicateKeySeed {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DuplicateKeyVisitor)
    }
}

fn reject_duplicate_keys(input: &[u8]) -> Result<(), serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_slice(input);
    deserializer.deserialize_any(DuplicateKeyVisitor)?;
    deserializer.end()
}

/// Output format accepted by both the configuration file and the CLI.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OutputFormat {
    #[default]
    Text,
    Json,
}

/// Missing-evidence behavior accepted by the declarative configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum MissingEvidencePolicy {
    Error,
    ReportOnly,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportConfig {
    #[serde(default, deserialize_with = "reject_null")]
    format: Option<OutputFormat>,
    #[serde(default, deserialize_with = "reject_null")]
    json: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageDetails {
    path: PathBuf,
    #[serde(default, deserialize_with = "reject_null")]
    format: Option<CoverageFormat>,
    #[serde(default, deserialize_with = "reject_null")]
    command: Option<CommandSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CoverageConfig {
    Path(PathBuf),
    Details(CoverageDetails),
}

type CoverageParts = (PathBuf, Option<CoverageFormat>, Option<Vec<String>>);

impl CoverageConfig {
    fn into_parts(self) -> Result<CoverageParts, String> {
        match self {
            Self::Path(path) => Ok((path, None, None)),
            Self::Details(details) => Ok((
                details.path,
                details.format,
                details.command.map(CommandSpec::into_argv).transpose()?,
            )),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PathList {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

impl PathList {
    fn into_vec(self) -> Vec<PathBuf> {
        match self {
            Self::One(path) => vec![path],
            Self::Many(paths) => paths,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThresholdOverride {
    path: String,
    threshold: u32,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ThresholdOverrides {
    Map(BTreeMap<String, u32>),
    List(Vec<ThresholdOverride>),
}

impl ThresholdOverrides {
    fn into_map(self) -> Result<BTreeMap<ProjectRelativePath, u32>, String> {
        let entries = match self {
            Self::Map(entries) => entries
                .into_iter()
                .map(|(path, threshold)| ThresholdOverride { path, threshold })
                .collect(),
            Self::List(entries) => entries,
        };
        let mut normalized = BTreeMap::new();
        for entry in entries {
            let path = ProjectRelativePath::new(&entry.path).map_err(|error| {
                format!("invalid threshold override path '{}': {error}", entry.path)
            })?;
            if normalized.insert(path.clone(), entry.threshold).is_some() {
                return Err(format!(
                    "duplicate threshold override path '{}' after normalization",
                    path
                ));
            }
        }
        Ok(normalized)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThresholdDetails {
    #[serde(default, deserialize_with = "reject_null")]
    global: Option<u32>,
    #[serde(default, deserialize_with = "reject_null")]
    default: Option<u32>,
    #[serde(default, deserialize_with = "reject_null")]
    paths: Option<ThresholdOverrides>,
    #[serde(default, deserialize_with = "reject_null")]
    overrides: Option<ThresholdOverrides>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ThresholdConfig {
    Overrides(ThresholdOverrides),
    Details(ThresholdDetails),
}

impl ThresholdConfig {
    fn into_parts(self) -> Result<(Option<u32>, BTreeMap<ProjectRelativePath, u32>), String> {
        match self {
            Self::Overrides(overrides) => Ok((None, overrides.into_map()?)),
            Self::Details(details) => {
                if details.paths.is_some() && details.overrides.is_some() {
                    return Err(
                        "thresholds.paths and thresholds.overrides are mutually exclusive"
                            .to_string(),
                    );
                }
                let global = match (details.global, details.default) {
                    (Some(_), Some(_)) => {
                        return Err(
                            "thresholds.global and thresholds.default are mutually exclusive"
                                .to_string(),
                        )
                    }
                    (Some(value), None) | (None, Some(value)) => Some(value),
                    (None, None) => None,
                };
                let overrides = details
                    .paths
                    .or(details.overrides)
                    .map_or_else(|| Ok(BTreeMap::new()), ThresholdOverrides::into_map)?;
                Ok((global, overrides))
            }
        }
    }
}

/// The closed schema for `crap4ts.json`.
///
/// Fields that are not represented here are rejected by serde's
/// `deny_unknown_fields`, including executable command fields.  Coverage
/// generation is represented by an explicit argv command, never executable
/// configuration code or an implicitly parsed shell string.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    #[serde(default, deserialize_with = "reject_null")]
    sources: Option<PathList>,
    #[serde(
        default,
        alias = "source-paths",
        alias = "sourcePaths",
        deserialize_with = "reject_null"
    )]
    source: Option<PathList>,
    #[serde(
        default,
        alias = "sourceRoots",
        alias = "source-roots",
        deserialize_with = "reject_null"
    )]
    source_roots: Option<PathList>,
    #[serde(default, deserialize_with = "reject_null")]
    coverage: Option<CoverageConfig>,
    #[serde(
        default,
        alias = "coverageFile",
        alias = "coverage-file",
        deserialize_with = "reject_null"
    )]
    coverage_file: Option<PathBuf>,
    #[serde(
        default,
        alias = "coverageFormat",
        alias = "coverage-format",
        deserialize_with = "reject_null"
    )]
    coverage_format: Option<CoverageFormat>,
    #[serde(
        default,
        alias = "coverageCommand",
        alias = "coverage-command",
        deserialize_with = "reject_null"
    )]
    coverage_command: Option<CommandSpec>,
    #[serde(default, deserialize_with = "reject_null")]
    command: Option<CommandSpec>,
    #[serde(
        default,
        alias = "generateCommand",
        alias = "generate-command",
        deserialize_with = "reject_null"
    )]
    generate_command: Option<CommandSpec>,
    #[serde(default, deserialize_with = "reject_null")]
    format: Option<OutputFormat>,
    #[serde(
        default,
        alias = "reportFormat",
        alias = "report-format",
        deserialize_with = "reject_null"
    )]
    report_format: Option<OutputFormat>,
    #[serde(default, deserialize_with = "reject_null")]
    json: Option<bool>,
    #[serde(default, alias = "output", deserialize_with = "reject_null")]
    report: Option<ReportConfig>,
    #[serde(default, deserialize_with = "reject_null")]
    reports: Option<ReportConfig>,
    #[serde(default, deserialize_with = "reject_null")]
    threshold: Option<u32>,
    #[serde(
        default,
        alias = "threshold-overrides",
        deserialize_with = "reject_null"
    )]
    thresholds: Option<ThresholdConfig>,
    #[serde(
        default,
        alias = "thresholdOverrides",
        deserialize_with = "reject_null"
    )]
    threshold_overrides: Option<ThresholdOverrides>,
    #[serde(
        default,
        alias = "reportOnly",
        alias = "report-only",
        deserialize_with = "reject_null"
    )]
    report_only: Option<bool>,
    #[serde(
        default,
        alias = "missingEvidence",
        alias = "missing-evidence",
        deserialize_with = "reject_null"
    )]
    missing_evidence: Option<MissingEvidencePolicy>,
    #[serde(
        default,
        alias = "missingCoverage",
        alias = "missing-coverage",
        deserialize_with = "reject_null"
    )]
    missing_coverage: Option<MissingEvidencePolicy>,
}

/// Values loaded from configuration before explicit CLI precedence is applied.
#[derive(Debug, Default)]
pub(crate) struct ConfigValues {
    pub(crate) sources: Option<Vec<PathBuf>>,
    pub(crate) coverage: Option<PathBuf>,
    pub(crate) coverage_format: Option<CoverageFormat>,
    pub(crate) coverage_command: Option<Vec<String>>,
    pub(crate) format: Option<OutputFormat>,
    pub(crate) json: Option<bool>,
    pub(crate) threshold: Option<u32>,
    pub(crate) threshold_overrides: BTreeMap<ProjectRelativePath, u32>,
    pub(crate) report_only: Option<bool>,
}

impl FileConfig {
    fn into_values(self) -> Result<ConfigValues, String> {
        let sources = match (self.sources, self.source, self.source_roots) {
            (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (_, Some(_), Some(_)) => {
                return Err("sources, source, and source_roots are mutually exclusive".to_string())
            }
            (Some(paths), None, None) | (None, Some(paths), None) => Some(paths.into_vec()),
            (None, None, Some(paths)) => Some(paths.into_vec()),
            (None, None, None) => None,
        };

        let (coverage, coverage_format_from_coverage, coverage_command_from_coverage) =
            match self.coverage {
                Some(coverage) => {
                    let (path, format, command) = coverage.into_parts()?;
                    (Some(path), format, command)
                }
                None => (None, None, None),
            };
        if coverage.is_some() && self.coverage_file.is_some() {
            return Err("coverage and coverage_file are mutually exclusive".to_string());
        }
        let coverage = coverage.or(self.coverage_file);
        let coverage_format = match (coverage_format_from_coverage, self.coverage_format) {
            (Some(_), Some(_)) => {
                return Err("coverage.format and coverage_format are mutually exclusive".to_string())
            }
            (Some(format), None) | (None, Some(format)) => Some(format),
            (None, None) => None,
        };

        let top_level_coverage_command = self
            .coverage_command
            .map(CommandSpec::into_argv)
            .transpose()?;
        let top_level_command = self.command.map(CommandSpec::into_argv).transpose()?;
        let top_level_generate_command = self
            .generate_command
            .map(CommandSpec::into_argv)
            .transpose()?;
        let coverage_command = match (
            coverage_command_from_coverage,
            top_level_coverage_command,
            top_level_command,
            top_level_generate_command,
        ) {
            (Some(_), Some(_), _, _)
            | (Some(_), _, Some(_), _)
            | (Some(_), _, _, Some(_))
            | (_, Some(_), Some(_), _)
            | (_, Some(_), _, Some(_))
            | (_, _, Some(_), Some(_)) => {
                return Err(
                    "coverage.command, coverage_command, command, and generate_command are mutually exclusive"
                        .to_string(),
                )
            }
            (Some(command), None, None, None)
            | (None, Some(command), None, None)
            | (None, None, Some(command), None)
            | (None, None, None, Some(command)) => Some(command),
            (None, None, None, None) => None,
        };

        let (format_from_report, json_from_report) = match (self.report, self.reports) {
            (Some(_), Some(_)) => {
                return Err("report and reports are mutually exclusive".to_string())
            }
            (Some(report), None) | (None, Some(report)) => (report.format, report.json),
            (None, None) => (None, None),
        };
        if self.format.is_some() && (format_from_report.is_some() || self.report_format.is_some()) {
            return Err("format and report format are mutually exclusive".to_string());
        }
        if self.json.is_some() && json_from_report.is_some() {
            return Err("json and report.json are mutually exclusive".to_string());
        }
        if format_from_report.is_some() && self.report_format.is_some() {
            return Err("report.format and report_format are mutually exclusive".to_string());
        }
        let format = self.format.or(format_from_report).or(self.report_format);
        let json = self.json.or(json_from_report);
        if format == Some(OutputFormat::Text) && json == Some(true) {
            return Err("format=text conflicts with json=true".to_string());
        }

        let report_only_from_missing = match (self.missing_evidence, self.missing_coverage) {
            (Some(_), Some(_)) => {
                return Err(
                    "missing_evidence and missing_coverage are mutually exclusive".to_string(),
                )
            }
            (Some(policy), None) | (None, Some(policy)) => {
                let value = matches!(policy, MissingEvidencePolicy::ReportOnly);
                Some(value)
            }
            (None, None) => None,
        };
        if self.report_only.is_some() && report_only_from_missing.is_some() {
            return Err(
                "report_only and missing_evidence/missing_coverage are mutually exclusive"
                    .to_string(),
            );
        }
        let report_only = self.report_only.or(report_only_from_missing);

        let mut threshold = self.threshold;
        let mut threshold_overrides = BTreeMap::new();
        if let Some(entries) = self.thresholds {
            let (global, overrides) = entries.into_parts()?;
            if threshold.is_some() && global.is_some() {
                return Err(
                    "threshold and thresholds.global/default are mutually exclusive".to_string(),
                );
            }
            threshold = threshold.or(global);
            threshold_overrides.extend(overrides);
        }
        if let Some(entries) = self.threshold_overrides {
            for (path, threshold) in entries.into_map()? {
                if threshold_overrides
                    .insert(path.clone(), threshold)
                    .is_some()
                {
                    return Err(format!("duplicate threshold override path '{path}'"));
                }
            }
        }

        Ok(ConfigValues {
            sources,
            coverage,
            coverage_format,
            coverage_command,
            format,
            json,
            threshold,
            threshold_overrides,
            report_only,
        })
    }
}

/// Discover the project-local configuration file. The first existing name in
/// this list wins, which makes discovery deterministic across projects that
/// happen to contain more than one supported spelling.
pub(crate) fn discover(root: &Path) -> Result<Option<PathBuf>, String> {
    for name in [
        DEFAULT_CONFIG_FILE,
        ".crap4ts.json",
        "crap4ts.config.json",
        ".crap4ts.config.json",
    ] {
        let candidate = root.join(name);
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "configuration: config path '{}' must not be a symlink",
                    candidate.display()
                ));
            }
            Ok(_) => return Ok(Some(candidate)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "configuration: unable to inspect config path '{}': {error}",
                    candidate.display()
                ));
            }
        }
    }
    Ok(None)
}

/// Read a project-local, data-only JSON configuration file.
pub(crate) fn load(path: &Path, root: &Path) -> Result<ConfigValues, String> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        format!(
            "configuration: unable to resolve config '{}': {error}",
            path.display()
        )
    })?;
    if !canonical.starts_with(root) {
        return Err(format!(
            "configuration: config '{}' escapes project root",
            path.display()
        ));
    }
    let metadata = fs::metadata(&canonical).map_err(|error| {
        format!(
            "configuration: unable to inspect config '{}': {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!(
            "configuration: config '{}' is not a regular file",
            path.display()
        ));
    }
    match canonical
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("json") => {}
        Some("js" | "mjs" | "cjs" | "ts") => {
            return Err(format!(
                "configuration: executable config '{}' is not supported; use JSON data",
                path.display()
            ));
        }
        _ => {
            return Err(format!(
                "configuration: unsupported config format for '{}' (expected .json)",
                path.display()
            ));
        }
    }
    let bytes = fs::read(&canonical).map_err(|error| {
        format!(
            "configuration: unable to read config '{}': {error}",
            path.display()
        )
    })?;
    reject_duplicate_keys(&bytes).map_err(|error| {
        format!(
            "configuration: invalid declarative config '{}': {error}",
            path.display()
        )
    })?;
    let parsed: FileConfig = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "configuration: invalid declarative config '{}': {error}",
            path.display()
        )
    })?;
    parsed.into_values().map_err(|error| {
        format!(
            "configuration: invalid declarative config '{}': {error}",
            path.display()
        )
    })
}

/// Resolve a config threshold map into the core's policy type.
pub(crate) fn policy(values: &ConfigValues, global: u32) -> ThresholdPolicy {
    values
        .threshold_overrides
        .iter()
        .fold(ThresholdPolicy::new(global), |policy, (path, threshold)| {
            policy.clone().with_path_override(path.clone(), *threshold)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fields_are_rejected_by_closed_schema() {
        let error =
            serde_json::from_str::<FileConfig>(r#"{"coverage":"coverage.json","run":"echo"}"#)
                .expect_err("unknown executable field must fail");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn duplicate_json_keys_are_rejected_before_schema_decoding() {
        let error = reject_duplicate_keys(br#"{"threshold":1,"threshold":2}"#)
            .expect_err("duplicate keys must fail");
        assert!(error.to_string().contains("duplicate JSON object key"));
    }

    #[test]
    fn explicit_null_does_not_activate_a_defaulted_option() {
        let error = serde_json::from_str::<FileConfig>(r#"{"threshold":null}"#)
            .expect_err("null threshold must fail");
        assert!(error.to_string().contains("null is not permitted"));
    }

    #[test]
    fn threshold_paths_are_normalized_and_sorted() {
        let config: FileConfig =
            serde_json::from_str(r#"{"thresholds":{"src\\nested.ts":3,"src/a.ts":2}}"#).unwrap();
        let values = config.into_values().unwrap();
        let paths = values
            .threshold_overrides
            .keys()
            .map(ProjectRelativePath::as_str)
            .collect::<Vec<_>>();
        assert_eq!(paths, ["src/a.ts", "src/nested.ts"]);
    }

    #[test]
    fn nested_threshold_settings_accept_global_and_path_values() {
        let config: FileConfig =
            serde_json::from_str(r#"{"thresholds":{"default":5,"paths":{"src/a.ts":2}}}"#).unwrap();
        let values = config.into_values().unwrap();
        assert_eq!(values.threshold, Some(5));
        assert_eq!(values.threshold_overrides.len(), 1);
        assert_eq!(
            values
                .threshold_overrides
                .get(&ProjectRelativePath::new("src/a.ts").unwrap()),
            Some(&2)
        );
    }

    #[test]
    fn generated_coverage_command_is_decoded_as_an_argv() {
        let config: FileConfig = serde_json::from_str(
            r#"{"coverage":{"path":"coverage.json","command":["npm","test","--","--coverage"]}}"#,
        )
        .unwrap();
        let values = config.into_values().unwrap();
        assert_eq!(
            values.coverage_command,
            Some(vec![
                "npm".to_string(),
                "test".to_string(),
                "--".to_string(),
                "--coverage".to_string()
            ])
        );
    }

    #[test]
    fn generated_coverage_command_object_preserves_argument_boundaries() {
        let config: FileConfig = serde_json::from_str(
            r#"{"coverage":"coverage.json","coverage_command":{"program":"node","args":["script with spaces.js","--flag=value"]}}"#,
        )
        .unwrap();
        let values = config.into_values().unwrap();
        assert_eq!(
            values.coverage_command,
            Some(vec![
                "node".to_string(),
                "script with spaces.js".to_string(),
                "--flag=value".to_string()
            ])
        );
    }

    #[test]
    fn generated_command_string_is_not_implicitly_a_shell_command() {
        let error = serde_json::from_str::<FileConfig>(
            r#"{"coverage":{"path":"coverage.json","command":"npm test"}}"#,
        )
        .expect_err("command strings must be rejected");
        assert!(!error.to_string().is_empty());
    }
}
