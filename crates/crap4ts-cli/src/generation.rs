//! Safe coverage generation at the CLI process boundary.
//!
//! Generation deliberately accepts an argument vector instead of a command
//! string.  The configured executable is passed directly to
//! [`std::process::Command`], so shell metacharacters are never interpreted by
//! crap4ts itself.  The configured artifact is removed before starting the
//! command and must be recreated successfully before its contents are read.

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Output},
};

use serde::Deserialize;

/// Declarative command configuration.
///
/// The short form is an argv array, for example `['npm', 'test']`.  The
/// object form is useful when a config author wants to make the program and
/// arguments visually distinct: `{ "program": "npm", "args": ["test"] }`.
/// Neither form invokes a shell.  A shell can still be used intentionally by
/// declaring it as the program, such as `['sh', '-c', '...']`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub(crate) enum CommandSpec {
    Argv(Vec<String>),
    Details(CommandDetails),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CommandDetails {
    #[serde(alias = "executable")]
    pub(crate) program: String,
    #[serde(default, alias = "arguments")]
    pub(crate) args: Vec<String>,
}

impl CommandSpec {
    pub(crate) fn into_argv(self) -> Result<Vec<String>, String> {
        let argv = match self {
            Self::Argv(argv) => argv,
            Self::Details(details) => {
                let mut argv = Vec::with_capacity(details.args.len() + 1);
                argv.push(details.program);
                argv.extend(details.args);
                argv
            }
        };
        validate_argv(&argv)?;
        Ok(argv)
    }
}

/// Run a configured coverage command and return the freshly generated UTF-8
/// artifact.  All child stdout and stderr bytes are forwarded to the parent's
/// stderr.  Keeping both pipes drained by `wait_with_output` avoids a child
/// deadlock when a runner emits more than a pipe's capacity.
pub(crate) fn generate(
    root: &Path,
    configured_artifact: &Path,
    argv: &[String],
) -> Result<String, String> {
    validate_argv(argv)?;
    let artifact = validate_artifact_path(root, configured_artifact)?;
    remove_existing_artifact(root, &artifact)?;

    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).current_dir(root);
    let output = command
        .output()
        .map_err(|error| format!("coverage command failed to start '{}': {error}", argv[0]))?;
    forward_output(&output).map_err(|error| {
        format!("coverage command output could not be forwarded to stderr: {error}")
    })?;
    if !output.status.success() {
        return Err(format!(
            "coverage command failed with {}",
            status_description(&output)
        ));
    }

    read_fresh_artifact(root, &artifact)
}

fn validate_argv(argv: &[String]) -> Result<(), String> {
    if argv.is_empty() {
        return Err("configuration: coverage command must contain a program".to_string());
    }
    if argv[0].is_empty() {
        return Err("configuration: coverage command program must not be empty".to_string());
    }
    if argv.iter().any(|argument| argument.contains('\0')) {
        return Err("configuration: coverage command arguments must not contain NUL".to_string());
    }
    Ok(())
}

fn status_description(output: &Output) -> String {
    output.status.code().map_or_else(
        || "termination by signal".to_string(),
        |code| format!("exit status {code}"),
    )
}

fn forward_output(output: &Output) -> io::Result<()> {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    // Preserve bytes exactly.  Child stdout is intentionally sent to the
    // parent's stderr so JSON stdout remains a single report document.
    handle.write_all(&output.stdout)?;
    handle.write_all(&output.stderr)?;
    handle.flush()
}

fn read_fresh_artifact(root: &Path, artifact: &Path) -> Result<String, String> {
    validate_artifact_path(root, artifact)?;
    let metadata = fs::symlink_metadata(artifact).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            format!(
                "coverage command succeeded but did not produce fresh coverage artifact '{}'",
                artifact.display()
            )
        } else {
            format!(
                "coverage command produced an unreadable coverage artifact '{}': {error}",
                artifact.display()
            )
        }
    })?;
    validate_artifact_metadata(artifact, &metadata)?;

    // Open only after the symlink/regular-file checks.  This is still a
    // normal filesystem path operation (rather than a recursive or shell
    // operation), and the pre-open metadata check keeps accidental symlink
    // artifacts from being accepted.
    let mut file = File::open(artifact).map_err(|error| {
        format!(
            "coverage command produced an unreadable coverage artifact '{}': {error}",
            artifact.display()
        )
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| {
        format!(
            "coverage command produced an unreadable coverage artifact '{}': {error}",
            artifact.display()
        )
    })?;
    String::from_utf8(bytes).map_err(|error| {
        format!(
            "coverage command produced a non-UTF-8 coverage artifact '{}': {error}",
            artifact.display()
        )
    })
}

fn remove_existing_artifact(root: &Path, artifact: &Path) -> Result<(), String> {
    // Re-validate immediately before deletion.  In particular, never turn a
    // path that became a symlink or directory into a recursive cleanup.
    validate_artifact_path(root, artifact)?;
    let metadata = match fs::symlink_metadata(artifact) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "unsafe coverage artifact path '{}': unable to inspect existing artifact: {error}",
                artifact.display()
            ));
        }
    };
    validate_artifact_metadata(artifact, &metadata)?;

    // A second metadata lookup is intentional: the target must still be the
    // same kind of non-symlink regular file at the point of deletion.
    let metadata = fs::symlink_metadata(artifact).map_err(|error| {
        format!(
            "unsafe coverage artifact path '{}': unable to recheck existing artifact: {error}",
            artifact.display()
        )
    })?;
    validate_artifact_metadata(artifact, &metadata)?;
    fs::remove_file(artifact).map_err(|error| {
        format!(
            "unsafe coverage artifact path '{}': unable to remove existing artifact: {error}",
            artifact.display()
        )
    })
}

fn validate_artifact_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "unsafe coverage artifact path '{}': artifact must not be a symlink",
            path.display()
        ));
    }
    if metadata.is_dir() {
        return Err(format!(
            "unsafe coverage artifact path '{}': artifact must be a regular file, not a directory",
            path.display()
        ));
    }
    if !metadata.is_file() {
        return Err(format!(
            "unsafe coverage artifact path '{}': artifact must be a regular file",
            path.display()
        ));
    }
    Ok(())
}

/// Resolve and validate a configured artifact path without following any
/// symlink in its existing path.  Missing final components are allowed (the
/// configured command may create them), but the nearest existing parent must
/// remain within the canonical project root.
fn validate_artifact_path(root: &Path, configured: &Path) -> Result<PathBuf, String> {
    let root = fs::canonicalize(root).map_err(|error| {
        format!(
            "unsafe coverage artifact path '{}': unable to resolve project root: {error}",
            root.display()
        )
    })?;
    let root_metadata = fs::metadata(&root).map_err(|error| {
        format!(
            "unsafe coverage artifact path '{}': unable to inspect project root: {error}",
            root.display()
        )
    })?;
    if !root_metadata.is_dir() {
        return Err(format!(
            "unsafe coverage artifact path '{}': project root is not a directory",
            root.display()
        ));
    }

    let configured_text = configured.to_str().ok_or_else(|| {
        format!(
            "unsafe coverage artifact path '{}': path must be valid UTF-8",
            configured.display()
        )
    })?;
    if configured_text.is_empty() || configured_text.contains('\0') {
        return Err(format!(
            "unsafe coverage artifact path '{}': path is empty or contains NUL",
            configured.display()
        ));
    }
    let lexical = configured_text.replace('\\', "/");
    if lexical.split('/').any(|component| component == "..") {
        return Err(format!(
            "unsafe coverage artifact path '{}': parent traversal is not allowed",
            configured.display()
        ));
    }
    if !configured.is_absolute()
        && (lexical.starts_with("//")
            || lexical.as_bytes().get(1) == Some(&b':')
            || lexical.starts_with('/'))
    {
        return Err(format!(
            "unsafe coverage artifact path '{}': absolute or drive-prefixed path is invalid",
            configured.display()
        ));
    }

    // Relative paths accept either separator so configuration authored on a
    // different host has deterministic behavior.
    let configured = if configured.is_absolute() {
        configured.to_path_buf()
    } else if std::path::MAIN_SEPARATOR == '/' {
        PathBuf::from(lexical)
    } else {
        PathBuf::from(lexical.replace('/', "\\"))
    };
    let artifact = if configured.is_absolute() {
        configured
    } else {
        root.join(configured)
    };
    if !artifact.starts_with(&root) || is_root_path(&artifact, &root) {
        return Err(format!(
            "unsafe coverage artifact path '{}': path must name a non-root project-local artifact",
            artifact.display()
        ));
    }

    validate_existing_ancestors(&root, &artifact)?;
    let nearest_parent = nearest_existing_parent(&artifact).ok_or_else(|| {
        format!(
            "unsafe coverage artifact path '{}': no existing project parent",
            artifact.display()
        )
    })?;
    let canonical_parent = fs::canonicalize(&nearest_parent).map_err(|error| {
        format!(
            "unsafe coverage artifact path '{}': unable to resolve nearest existing parent '{}': {error}",
            artifact.display(),
            nearest_parent.display()
        )
    })?;
    if !canonical_parent.starts_with(&root) {
        return Err(format!(
            "unsafe coverage artifact path '{}': nearest existing parent escapes project root",
            artifact.display()
        ));
    }
    Ok(artifact)
}

fn is_root_path(path: &Path, root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    relative.components().all(|component| {
        matches!(
            component,
            Component::CurDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

fn nearest_existing_parent(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    loop {
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() => return Some(current),
            Ok(_) => return current.parent().map(Path::to_path_buf),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                current = current.parent()?.to_path_buf();
            }
            Err(_) => return None,
        }
    }
}

fn validate_existing_ancestors(root: &Path, artifact: &Path) -> Result<(), String> {
    let relative = artifact.strip_prefix(root).map_err(|_| {
        format!(
            "unsafe coverage artifact path '{}': path escapes project root",
            artifact.display()
        )
    })?;
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => components.push(name),
            Component::CurDir => {}
            _ => {
                return Err(format!(
                    "unsafe coverage artifact path '{}': unsupported path component",
                    artifact.display()
                ));
            }
        }
    }
    let mut current = root.to_path_buf();
    for (index, name) in components.iter().enumerate() {
        current.push(name);
        let is_target = index == components.len() - 1;
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "unsafe coverage artifact path '{}': symlink ancestor '{}' is not allowed",
                    artifact.display(),
                    current.display()
                ));
            }
            Ok(metadata) if !is_target && !metadata.is_dir() => {
                return Err(format!(
                    "unsafe coverage artifact path '{}': parent '{}' is not a directory",
                    artifact.display(),
                    current.display()
                ));
            }
            Ok(metadata) if is_target => validate_artifact_metadata(&current, &metadata)?,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(format!(
                    "unsafe coverage artifact path '{}': unable to inspect '{}': {error}",
                    artifact.display(),
                    current.display()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

    fn fixture() -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("crap4ts-generation-{}-{id}", std::process::id()));
        fs::create_dir_all(root.join("nested")).unwrap();
        root
    }

    #[test]
    fn command_spec_is_an_argv_without_shell_splitting() {
        let spec = CommandSpec::Argv(vec![
            "tool with spaces".to_string(),
            "argument;not-shell-code".to_string(),
        ]);
        assert_eq!(
            spec.into_argv().unwrap(),
            ["tool with spaces", "argument;not-shell-code"]
        );
    }

    #[test]
    fn missing_target_is_accepted_only_under_existing_parent() {
        let root = fixture();
        let target = root.join("nested/coverage.json");
        assert_eq!(
            validate_artifact_path(&root, Path::new("nested/coverage.json")).unwrap(),
            target
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn root_and_traversal_are_rejected() {
        let root = fixture();
        assert!(validate_artifact_path(&root, Path::new(".")).is_err());
        assert!(validate_artifact_path(&root, Path::new("../coverage.json")).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
