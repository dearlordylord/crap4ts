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
        let boundary = if path.is_dir() {
            path.clone()
        } else {
            root.clone()
        };
        if path.is_file() {
            validate_explicit_file(&root, &path)?;
        }
        collect_path(&root, &boundary, &path, &mut files)?;
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

    // Keep the operating system's native path representation intact. In
    // particular, Windows drive and verbatim prefixes must reach canonicalize
    // unchanged; separator normalization belongs only to project identities.
    let path = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
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

fn collect_path(
    project_root: &Path,
    boundary: &Path,
    path: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<(), CoreError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() {
        let target = fs::canonicalize(path).map_err(io_error)?;
        if !target.starts_with(boundary) {
            return Err(CoreError::SourceSelection(format!(
                "symlink '{}' escapes selected source root",
                path.display()
            )));
        }
        if excluded_project_directory(project_root, &target) {
            return Ok(());
        }
        if target.is_dir() {
            return Err(CoreError::SourceSelection(format!(
                "symlink directory '{}' is not allowed",
                path.display()
            )));
        }
        return collect_path(project_root, boundary, &target, files);
    }

    if excluded_project_directory(project_root, path) {
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
            collect_path(project_root, boundary, &child.path(), files)?;
        }
        return Ok(());
    }

    if !metadata.is_file() || !is_typescript(path) || is_declaration(path) || is_test_file(path) {
        return Ok(());
    }
    let relative = path.strip_prefix(project_root).map_err(|_| {
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

fn validate_explicit_file(project_root: &Path, path: &Path) -> Result<(), CoreError> {
    if !is_typescript(path) {
        return Err(CoreError::SourceSelection(format!(
            "explicit source file '{}' has an unsupported extension",
            path.display()
        )));
    }
    if excluded_project_directory(project_root, path) || is_declaration(path) || is_test_file(path)
    {
        return Err(CoreError::SourceSelection(format!(
            "explicit source file '{}' is excluded",
            path.display()
        )));
    }
    Ok(())
}

fn excluded_project_directory(project_root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(project_root) else {
        return false;
    };
    relative
        .components()
        .any(|component| matches!(component, Component::Normal(name) if excluded_directory(name)))
}

fn reject_symlink_directories(path: &Path) -> Result<(), CoreError> {
    // Ancestors retain native prefixes (including Windows drive and verbatim
    // prefixes), unlike rebuilding a path one `Component` at a time.
    let mut ancestors = path.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    for current in ancestors {
        let metadata = fs::symlink_metadata(current).map_err(io_error)?;
        if metadata.file_type().is_symlink() {
            let target = fs::canonicalize(current).map_err(io_error)?;
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

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn drive_and_verbatim_paths_keep_native_prefixes() {
        let drive = PathBuf::from(r"C:\project");
        assert!(drive.is_absolute());
        let drive_file = drive.join(r"src\file.ts");
        assert_eq!(drive_file.to_str(), Some(r"C:\project\src\file.ts"));

        let verbatim = PathBuf::from(r"\\?\C:\project");
        assert!(verbatim.is_absolute());
        let verbatim_file = verbatim.join(r"src\file.ts");
        assert_eq!(verbatim_file.to_str(), Some(r"\\?\C:\project\src\file.ts"));
        assert!(verbatim_file.starts_with(&verbatim));
    }
}
