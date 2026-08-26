//! Filesystem-facing source selection.
//!
//! This boundary owns canonical project-relative identities and keeps source
//! discovery deterministic. It deliberately does not know anything about
//! coverage or scoring.

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs, io,
    path::{Component, Path, PathBuf},
};

use crate::domain::{CoreError, ProjectRelativePath, SourceFile};

/// Select TypeScript source files below a project root.
///
/// Inputs are de-duplicated and sorted by their canonical project-relative
/// identity. Symlink directories are rejected rather than followed, which
/// makes cycles impossible and keeps traversal inside the project root.
///
/// An empty `requested` list means the whole project root. Every explicit
/// input must exist and be readable, while a directory that contains no
/// reportable files returns an empty selection for the caller to handle.
pub fn collect_sources(root: &Path, requested: &[PathBuf]) -> Result<Vec<SourceFile>, CoreError> {
    let root = canonicalize_root(root)?;
    let mut files = BTreeMap::new();
    let inputs = if requested.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        requested.to_vec()
    };

    for requested_path in inputs {
        let path = resolve_requested_path(&root, &requested_path)?;
        collect_path(&root, &path, &mut files)?;
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

fn canonicalize_root(root: &Path) -> Result<PathBuf, CoreError> {
    let root = fs::canonicalize(root).map_err(|error| {
        CoreError::SourceSelection(format!(
            "unable to resolve project root '{}': {error}",
            root.display()
        ))
    })?;
    let metadata = fs::metadata(&root).map_err(io_error)?;
    if !metadata.is_dir() {
        return Err(CoreError::SourceSelection(format!(
            "project root '{}' is not a directory",
            root.display()
        )));
    }
    Ok(root)
}

fn resolve_requested_path(root: &Path, requested: &Path) -> Result<PathBuf, CoreError> {
    if requested.as_os_str().is_empty() {
        return Err(CoreError::SourceSelection(
            "source path is empty".to_string(),
        ));
    }

    // Coverage tools and command-line users commonly use Windows separators
    // even when the surrounding tooling runs on POSIX. Normalize before
    // joining so that source selection has the same identity on both systems.
    let requested = normalize_separators(requested)?;
    let path = if requested.is_absolute() {
        requested
    } else {
        root.join(requested)
    };
    let lexical = normalize_path(&path);
    if !lexical.starts_with(root) {
        return Err(CoreError::SourceSelection(format!(
            "source path '{}' escapes project root",
            path.display()
        )));
    }
    let canonical = fs::canonicalize(&path).map_err(|error| {
        CoreError::SourceSelection(format!(
            "unable to resolve source '{}': {error}",
            path.display()
        ))
    })?;
    if !canonical.starts_with(root) {
        return Err(CoreError::SourceSelection(format!(
            "source path '{}' escapes project root",
            path.display()
        )));
    }

    // Canonicalizing the complete input would otherwise hide a symlinked
    // directory. Reject it before traversal, including a symlink that points
    // to a directory inside the project. Symlinked files are allowed only
    // when their resolved target remains within the root.
    reject_symlink_directories(&path)?;
    Ok(canonical)
}

fn normalize_separators(path: &Path) -> Result<PathBuf, CoreError> {
    let value = path.to_str().ok_or_else(|| {
        CoreError::SourceSelection(format!(
            "source path '{}' is not valid UTF-8",
            path.display()
        ))
    })?;
    Ok(PathBuf::from(value.replace('\\', "/")))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // `PathBuf::pop` leaves a filesystem root in place, which is
                // exactly the behavior needed for confinement checks.
                let _ = normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
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

    if metadata.is_dir() && path != root && path.file_name().is_some_and(excluded_directory) {
        return Ok(());
    }

    if metadata.is_dir() {
        let mut children = fs::read_dir(path)
            .map_err(io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(io_error)?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            // Do not skip symlinks solely because their name is excluded: a
            // symlink must still be resolved and rejected if it escapes the
            // project root. `file_type` does not follow symlinks.
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
        fs::read_to_string(path).map_err(|error| {
            CoreError::SourceSelection(format!(
                "unable to read source '{}': {error}",
                path.display()
            ))
        })?,
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
        path.extension().and_then(OsStr::to_str),
        Some("ts" | "tsx" | "mts" | "cts")
    )
}

fn is_declaration(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            name.ends_with(".d.ts") || name.ends_with(".d.mts") || name.ends_with(".d.cts")
        })
}

fn is_test_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    let stem = name
        .strip_suffix(".tsx")
        .or_else(|| name.strip_suffix(".ts"))
        .or_else(|| name.strip_suffix(".mts"))
        .or_else(|| name.strip_suffix(".cts"))
        .unwrap_or(&name);
    matches!(stem, "test" | "spec")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
}

fn excluded_directory(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
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
}
