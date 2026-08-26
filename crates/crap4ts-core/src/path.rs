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

/// Safe overrides for conventional source discovery filters. Repository
/// metadata and dependencies remain excluded regardless of these settings.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceSelectionOptions {
    pub include_tests: bool,
    pub include_generated: bool,
}

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
    collect_sources_with_options(root, requested, SourceSelectionOptions::default())
}

pub fn collect_sources_with_options(
    root: &Path,
    requested: &[PathBuf],
    options: SourceSelectionOptions,
) -> Result<Vec<SourceFile>, CoreError> {
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
            validate_explicit_file(&root, &path, options)?;
        }
        collect_path(&root, &boundary, &path, &mut files, options)?;
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

    // Keep absolute filesystem paths in their native representation. In
    // particular, Windows drive and verbatim prefixes must reach canonicalize
    // unchanged. Relative CLI/config paths may use either conventional
    // separator, so translate only those components before joining them to
    // the native project root.
    let requested = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        normalize_relative_path(requested)?
    };
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

fn normalize_relative_path(path: &Path) -> Result<PathBuf, CoreError> {
    let value = path.to_str().ok_or_else(|| {
        CoreError::SourceSelection(format!(
            "source path '{}' is not valid UTF-8",
            path.display()
        ))
    })?;
    let normalized = if std::path::MAIN_SEPARATOR == '/' {
        value.replace('\\', "/")
    } else {
        value.replace('/', "\\")
    };
    Ok(PathBuf::from(normalized))
}

fn collect_path(
    project_root: &Path,
    boundary: &Path,
    path: &Path,
    files: &mut BTreeMap<String, String>,
    options: SourceSelectionOptions,
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
        if excluded_project_directory(project_root, &target, options) {
            return Ok(());
        }
        if target.is_dir() {
            return Err(CoreError::SourceSelection(format!(
                "symlink directory '{}' is not allowed",
                path.display()
            )));
        }
        return collect_path(project_root, boundary, &target, files, options);
    }

    if excluded_project_directory(project_root, path, options) {
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
                && excluded_directory(&child.file_name(), options)
            {
                continue;
            }
            collect_path(project_root, boundary, &child.path(), files, options)?;
        }
        return Ok(());
    }

    if !metadata.is_file()
        || !is_typescript(path)
        || is_declaration(path)
        || (!options.include_tests && is_test_file(path))
    {
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

fn validate_explicit_file(
    project_root: &Path,
    path: &Path,
    options: SourceSelectionOptions,
) -> Result<(), CoreError> {
    if !is_typescript(path) {
        return Err(CoreError::SourceSelection(format!(
            "explicit source file '{}' has an unsupported extension",
            path.display()
        )));
    }
    if excluded_project_directory(project_root, path, options)
        || is_declaration(path)
        || (!options.include_tests && is_test_file(path))
    {
        return Err(CoreError::SourceSelection(format!(
            "explicit source file '{}' is excluded",
            path.display()
        )));
    }
    Ok(())
}

fn excluded_project_directory(
    project_root: &Path,
    path: &Path,
    options: SourceSelectionOptions,
) -> bool {
    let Ok(relative) = path.strip_prefix(project_root) else {
        return false;
    };
    relative
        .components()
        .any(|component| matches!(component, Component::Normal(name) if excluded_directory(name, options)))
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

fn excluded_directory(name: &OsStr, options: SourceSelectionOptions) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    matches!(name.as_str(), "node_modules" | ".git")
        || (!options.include_generated
            && matches!(name.as_str(), "target" | "dist" | "build" | "coverage"))
        || (!options.include_tests
            && matches!(name.as_str(), "test" | "tests" | "__tests__" | "__mocks__"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_filter_overrides_include_tests_and_generated_but_not_dependencies() {
        let root =
            std::env::temp_dir().join(format!("crap4ts-source-filters-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for directory in ["src", "tests", "dist", "node_modules/dependency"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        for file in [
            "src/main.ts",
            "tests/main.ts",
            "dist/generated.ts",
            "node_modules/dependency/index.ts",
        ] {
            fs::write(root.join(file), "export const value = () => 1;\n").unwrap();
        }

        let selected = collect_sources_with_options(
            &root,
            &[],
            SourceSelectionOptions {
                include_tests: true,
                include_generated: true,
            },
        )
        .unwrap();
        let identities = selected
            .iter()
            .map(|source| source.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            identities,
            ["dist/generated.ts", "src/main.ts", "tests/main.ts"]
        );
        fs::remove_dir_all(root).unwrap();
    }
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
