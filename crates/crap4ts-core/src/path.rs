//! Filesystem-facing source selection.
//!
//! This boundary owns canonical project-relative identities and keeps source
//! discovery deterministic. It deliberately does not know anything about
//! coverage or scoring.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use crate::domain::{CoreError, ProjectRelativePath, SourceFile};

/// Select TypeScript source files below a project root.
///
/// Inputs are de-duplicated and sorted by their canonical project-relative
/// identity. Symlink directories are rejected rather than followed, which
/// makes cycles impossible and keeps traversal inside the project root.
pub fn collect_sources(root: &Path, requested: &[PathBuf]) -> Result<Vec<SourceFile>, CoreError> {
    let root = fs::canonicalize(root)
        .map_err(|error| CoreError::SourceSelection(format!("unable to resolve root: {error}")))?;
    let mut files = BTreeMap::new();
    let inputs = if requested.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        requested.to_vec()
    };

    for requested_path in inputs {
        let path = if requested_path.is_absolute() {
            requested_path
        } else {
            root.join(requested_path)
        };
        let canonical = fs::canonicalize(&path).map_err(|error| {
            CoreError::SourceSelection(format!("unable to resolve '{}': {error}", path.display()))
        })?;
        if !canonical.starts_with(&root) {
            return Err(CoreError::SourceSelection(format!(
                "source path '{}' escapes project root",
                canonical.display()
            )));
        }
        reject_symlink_directories(&path)?;
        // Use the canonical spelling for identities and traversal. The
        // preflight above still observes symlinked directory components in an
        // explicitly requested path, so canonicalization cannot bypass the
        // symlink-directory policy.
        collect_path(&root, &canonical, &mut files)?;
    }

    files
        .into_iter()
        .map(|(path, source)| {
            let path = ProjectRelativePath::new(&path)
                .map_err(|error| CoreError::SourceSelection(error.to_string()))?;
            Ok(SourceFile { path, source })
        })
        .collect()
}

fn collect_path(
    root: &Path,
    path: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<(), CoreError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() {
        let target = fs::canonicalize(path).map_err(io_error)?;
        if !target.starts_with(root) {
            return Err(CoreError::SourceSelection(format!(
                "symlink '{}' escapes project root",
                path.display()
            )));
        }
        if target.is_dir() {
            return Err(CoreError::SourceSelection(format!(
                "symlink directory '{}' is not allowed",
                path.display()
            )));
        }
        return collect_path(root, &target, files);
    }

    if metadata.is_dir() {
        let mut children = fs::read_dir(path)
            .map_err(io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            if child.file_type().map_err(io_error)?.is_dir()
                && excluded_directory(&child.file_name())
            {
                continue;
            }
            collect_path(root, &child.path(), files)?;
        }
        return Ok(());
    }

    if !metadata.is_file() || !is_typescript(path) || is_declaration(path) || is_test_file(path) {
        return Ok(());
    }
    let relative = path.strip_prefix(root).map_err(|_| {
        CoreError::SourceSelection(format!("source '{}' escaped project root", path.display()))
    })?;
    let identity = relative.to_str().ok_or_else(|| {
        CoreError::SourceSelection(format!(
            "source path '{}' is not valid UTF-8",
            path.display()
        ))
    })?;
    files.insert(
        identity.replace('\\', "/"),
        fs::read_to_string(path).map_err(io_error)?,
    );
    Ok(())
}

fn reject_symlink_directories(path: &Path) -> Result<(), CoreError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(io_error)?;
        if metadata.file_type().is_symlink() {
            let target = fs::canonicalize(&current).map_err(io_error)?;
            if target.is_dir() {
                return Err(CoreError::SourceSelection(format!(
                    "symlink directory '{}' is not allowed",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn io_error(error: io::Error) -> CoreError {
    CoreError::SourceSelection(error.to_string())
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

fn is_test_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let stem = name
        .strip_suffix(".tsx")
        .or_else(|| name.strip_suffix(".ts"))
        .or_else(|| name.strip_suffix(".mts"))
        .or_else(|| name.strip_suffix(".cts"))
        .unwrap_or(name);
    matches!(stem, "test" | "spec")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
}

fn excluded_directory(name: &OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(
            "node_modules"
                | "target"
                | "dist"
                | "build"
                | "coverage"
                | ".git"
                | "test"
                | "tests"
                | "__tests__"
                | "__mocks__"
        )
    )
}
