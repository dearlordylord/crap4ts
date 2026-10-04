//! Git selection is an intersection with already validated source discovery.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let result = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|error| format!("configuration: cannot run git: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "configuration: git selection failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    Ok(result.stdout)
}

pub(crate) fn selected(
    root: &Path,
    changed: bool,
    base: Option<&str>,
) -> Result<Option<BTreeSet<PathBuf>>, String> {
    if !changed && base.is_none() {
        return Ok(None);
    }
    let top = git(root, &["rev-parse", "--show-toplevel"])?;
    let top = std::str::from_utf8(&top).map_err(|_| "configuration: git root is not UTF-8")?;
    // Remove Git's terminating newline without trimming spaces from the path.
    let top = Path::new(top.strip_suffix('\n').unwrap_or(top));
    // Match source discovery's canonical identities, including Windows verbatim
    // prefixes and filesystem aliases such as macOS /var -> /private/var.
    let top = std::fs::canonicalize(top)
        .map_err(|error| format!("configuration: cannot resolve git root: {error}"))?;
    let mut names = Vec::new();
    if let Some(base) = base {
        let revision = format!("{base}^{{commit}}");
        let commit = git(
            root,
            &["rev-parse", "--verify", "--end-of-options", &revision],
        )?;
        let commit = std::str::from_utf8(&commit)
            .map_err(|_| "configuration: invalid git commit")?
            .trim();
        names.extend(
            git(
                &top,
                &[
                    "diff",
                    "--name-only",
                    "--diff-filter=ACMRTU",
                    "--no-renames",
                    "-z",
                    commit,
                    "--",
                ],
            )?
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(Vec::from),
        );
        names.extend(
            git(&top, &["ls-files", "--others", "--exclude-standard", "-z"])?
                .split(|byte| *byte == 0)
                .filter(|name| !name.is_empty())
                .map(Vec::from),
        );
    } else {
        names.extend(
            git(
                &top,
                &[
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--no-renames",
                    "--untracked-files=all",
                ],
            )?
            .split(|byte| *byte == 0)
            .filter(|entry| entry.len() >= 4)
            .map(|entry| entry[3..].to_vec()),
        );
    }
    let mut paths = BTreeSet::new();
    for name in names {
        let name = std::str::from_utf8(&name)
            .map_err(|_| "configuration: changed filename is not UTF-8")?;
        let path = top.join(name);
        if path.starts_with(root) {
            match std::fs::canonicalize(&path) {
                Ok(path) if path.starts_with(root) => {
                    paths.insert(path);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "configuration: cannot resolve changed path '{}': {error}",
                        path.display()
                    ))
                }
            }
        }
    }
    Ok(Some(paths))
}
