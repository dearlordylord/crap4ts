use std::{fmt, path::Path};

use serde::{
    de::{self, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use thiserror::Error;

/// Version of the canonical JSON report document.
pub const REPORT_VERSION: u32 = 1;

/// A validated project-relative path. Absolute paths, parent traversal, and
/// platform-specific drive prefixes are rejected at this boundary.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProjectRelativePath(String);

impl ProjectRelativePath {
    pub fn new(input: &str) -> Result<Self, CoreError> {
        let normalized = input.replace('\\', "/");
        if normalized.is_empty()
            || normalized.starts_with('/')
            || normalized.starts_with("//")
            || normalized.as_bytes().get(1) == Some(&b':')
            || normalized.as_bytes().contains(&0)
        {
            return Err(CoreError::InvalidProjectPath(input.to_string()));
        }
        let mut components = Vec::new();
        for component in normalized.split('/') {
            match component {
                "" | "." => {}
                ".." => return Err(CoreError::InvalidProjectPath(input.to_string())),
                component => components.push(component),
            }
        }
        if components.is_empty() {
            return Err(CoreError::InvalidProjectPath(input.to_string()));
        }
        Ok(Self(components.join("/")))
    }

    pub fn from_filesystem_path(path: &Path, root: &Path) -> Result<Self, CoreError> {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| CoreError::InvalidProjectPath(path.display().to_string()))?;
        let value = relative
            .to_str()
            .ok_or_else(|| CoreError::InvalidProjectPath(path.display().to_string()))?;
        Self::new(value)
    }

    pub fn from_artifact_path(input: &str, root: &Path) -> Result<Self, CoreError> {
        let normalized = input.replace('\\', "/");
        let candidate = Path::new(&normalized);
        if candidate.is_absolute() {
            let canonical_root = std::fs::canonicalize(root)
                .map_err(|_| CoreError::InvalidProjectPath(input.to_string()))?;
            let canonical = std::fs::canonicalize(candidate)
                .map_err(|_| CoreError::InvalidProjectPath(input.to_string()))?;
            return Self::from_filesystem_path(&canonical, &canonical_root);
        }
        Self::new(&normalized)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectRelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ProjectRelativePath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ProjectRelativePath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PathVisitor;

        impl<'de> Visitor<'de> for PathVisitor {
            type Value = ProjectRelativePath;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a project-relative path")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                ProjectRelativePath::new(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(PathVisitor)
    }
}

/// A source file selected within the project root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFile {
    pub path: ProjectRelativePath,
    pub source: String,
}

/// A byte offset and human-readable source position.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourcePosition {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

/// An inclusive-start, exclusive-end source range.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl SourceRange {
    pub(crate) fn size(self) -> usize {
        self.end.offset.saturating_sub(self.start.offset)
    }
}

/// Concrete executable function-like units recognized by the source adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionKind {
    Arrow,
    Constructor,
    FunctionDeclaration,
    FunctionExpression,
    Getter,
    Method,
    Setter,
}

impl FunctionKind {
    pub(crate) fn as_label(self) -> &'static str {
        match self {
            Self::Arrow => "arrow",
            Self::Constructor => "constructor",
            Self::FunctionDeclaration => "function declaration",
            Self::FunctionExpression => "function expression",
            Self::Getter => "getter",
            Self::Method => "method",
            Self::Setter => "setter",
        }
    }
}

/// Validated cyclomatic complexity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct Complexity(u32);

impl Complexity {
    pub fn new(value: u32) -> Result<Self, CoreError> {
        if value == 0 {
            return Err(CoreError::InvalidComplexity(value));
        }
        Ok(Self(value))
    }

    pub const fn one() -> Self {
        Self(1)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl<'de> Deserialize<'de> for Complexity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u32::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// Measured coverage, or explicit unavailable evidence.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Coverage {
    Measured {
        covered: u64,
        total: u64,
        fraction: f64,
    },
    Unknown {
        reason: String,
    },
}

impl Coverage {
    pub fn measured(covered: u64, total: u64) -> Result<Self, CoreError> {
        if total == 0 || covered > total {
            return Err(CoreError::InvalidCoverage { covered, total });
        }
        Ok(Self::Measured {
            covered,
            total,
            fraction: covered as f64 / total as f64,
        })
    }

    pub fn unknown(reason: impl Into<String>) -> Self {
        Self::Unknown {
            reason: reason.into(),
        }
    }

    pub fn fraction(&self) -> Option<f64> {
        match self {
            Self::Measured { fraction, .. } => Some(*fraction),
            Self::Unknown { .. } => None,
        }
    }
}

/// A normalized source function. Oxc AST nodes never cross this boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FunctionUnit {
    pub id: String,
    pub path: ProjectRelativePath,
    pub name: String,
    pub kind: FunctionKind,
    pub range: SourceRange,
    pub body_range: SourceRange,
    pub complexity: Complexity,
}

/// A scored or unavailable report row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReportRow {
    pub id: String,
    pub path: ProjectRelativePath,
    pub name: String,
    pub kind: FunctionKind,
    pub range: SourceRange,
    /// Internal body boundary used by coverage attribution. The public report
    /// keeps the stable function range from the original v1 schema.
    #[serde(skip_serializing)]
    pub body_range: SourceRange,
    pub complexity: Complexity,
    pub coverage: Coverage,
    pub crap: Option<f64>,
}

/// Structured report diagnostic categories.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCategory {
    Configuration,
    CoverageAttribution,
    CoverageParsing,
    MissingEvidence,
    SourceParsing,
    ThresholdBreach,
}

impl DiagnosticCategory {
    pub(crate) const fn as_label(self) -> &'static str {
        match self {
            Self::Configuration => "configuration",
            Self::CoverageAttribution => "coverage_attribution",
            Self::CoverageParsing => "coverage_parsing",
            Self::MissingEvidence => "missing_evidence",
            Self::SourceParsing => "source_parsing",
            Self::ThresholdBreach => "threshold_breach",
        }
    }
}

/// Deterministic user-facing diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub category: DiagnosticCategory,
    pub message: String,
}

impl Diagnostic {
    pub(crate) fn new(category: DiagnosticCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

/// Canonical report document. It intentionally contains no timestamp.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub version: u32,
    pub threshold: u32,
    pub rows: Vec<ReportRow>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("unsupported source extension for '{0}' (expected .ts or .tsx)")]
    UnsupportedSource(String),
    #[error("source parsing failed for '{path}': {message}")]
    SourceParsing { path: String, message: String },
    #[error("coverage parsing failed: {0}")]
    CoverageParsing(String),
    #[error("coverage attribution failed: {0}")]
    CoverageAttribution(String),
    #[error("source selection failed: {0}")]
    SourceSelection(String),
    #[error("invalid project-relative path '{0}'")]
    InvalidProjectPath(String),
    #[error("missing coverage evidence for '{path}': {reason}")]
    MissingEvidence { path: String, reason: String },
    #[error("complexity must be positive, got {0}")]
    InvalidComplexity(u32),
    #[error("invalid coverage counts: covered={covered}, total={total}")]
    InvalidCoverage { covered: u64, total: u64 },
    #[error("invalid coverage fraction: {0}")]
    InvalidCoverageFraction(f64),
    #[error("coverage artifact contains ambiguous file identity '{0}'")]
    AmbiguousCoverageFile(String),
}
