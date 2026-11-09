//! Creating patches: a commit's diff, or the working tree's, as a `git apply`-ready file.

use crate::error::{GbError, GbErrorKind};
use crate::git::{GitCli, GitInvocation};
use serde::Serialize;
use std::path::Path;
use ts_rs::TS;

/// The largest patch the UI may hand back to `git apply` (a mail client's attachment is small;
/// this is only a guard against a runaway file).
pub const MAX_PATCH_BYTES: usize = 16 * 1024 * 1024;

/// A patch and the file name the save dialog suggests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct PatchPayload {
    pub name: String,
    pub text: String,
}

/// `git diff` of `rev` against its first parent (a root commit: `git show`), as a binary-capable
/// patch. A merge commit uses its first parent, so the result applies like any other diff.
pub(crate) async fn commit_patch(
    cli: &GitCli,
    root: &Path,
    rev: &str,
) -> Result<PatchPayload, GbError> {
    let rev = rev.trim();
    if rev.is_empty() || rev.starts_with('-') {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "Choose a commit to patch",
        ));
    }
    let full = run(
        cli,
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{rev}^{{commit}}"),
        ],
    )
    .await?;
    let sha = full.trim();
    if sha.is_empty() {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "That commit doesn't exist",
        ));
    }
    let parents = run(cli, root, &["rev-list", "--parents", "-n", "1", sha]).await?;
    let parent = parents.split_whitespace().nth(1);
    let text = match parent {
        Some(parent) => {
            run(
                cli,
                root,
                &[
                    "diff",
                    "--no-color",
                    "--no-ext-diff",
                    "--binary",
                    parent,
                    sha,
                ],
            )
            .await?
        }
        None => {
            run(
                cli,
                root,
                &[
                    "show",
                    "--format=",
                    "--patch",
                    "--no-color",
                    "--binary",
                    sha,
                ],
            )
            .await?
        }
    };
    if text.trim().is_empty() {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            "That commit changes no files",
        ));
    }
    let subject = run(cli, root, &["show", "-s", "--format=%s", sha]).await?;
    Ok(PatchPayload {
        name: patch_name(sha, &subject),
        text,
    })
}

/// `git diff` (or `git diff --cached`) of the working tree: a binary-capable patch. Untracked
/// files aren't in a diff; the caller's hint says so.
pub(crate) async fn worktree_patch(
    cli: &GitCli,
    root: &Path,
    staged: bool,
) -> Result<PatchPayload, GbError> {
    let mut args = vec!["diff"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--no-color", "--no-ext-diff", "--binary"]);
    let text = run(cli, root, &args).await?;
    if text.trim().is_empty() {
        return Err(GbError::new(
            GbErrorKind::InvalidInput,
            if staged {
                "There are no staged changes to patch"
            } else {
                "There are no unstaged changes to patch"
            },
        ));
    }
    let name = if staged {
        "staged-changes.patch"
    } else {
        "changes.patch"
    };
    Ok(PatchPayload {
        name: name.to_string(),
        text,
    })
}

async fn run(cli: &GitCli, root: &Path, args: &[&str]) -> Result<String, GbError> {
    let out = cli
        .run(GitInvocation::new(root, args.iter().copied()))
        .await?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `<short sha>-<subject slug>.patch`, safe as a file name and short enough to read.
fn patch_name(sha: &str, subject: &str) -> String {
    let short = &sha[..sha.len().min(7)];
    let mut slug = String::new();
    for c in subject.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 40 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        format!("{short}.patch")
    } else {
        format!("{short}-{slug}.patch")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitCli;
    use crate::log::CommandLog;
    use crate::testing::{TestRepo, isolated_git_env};
    use std::sync::Arc;

    fn cli() -> GitCli {
        GitCli::new(Arc::new(CommandLog::new(20))).with_env(isolated_git_env())
    }

    #[tokio::test]
    async fn a_commit_becomes_an_applicable_patch_named_after_it() {
        let repo = TestRepo::new();
        let sha = repo.commit("Add the widget");
        let patch = commit_patch(&cli(), repo.path(), &sha).await.unwrap();
        assert!(
            patch.text.contains("diff --git a/file_0.txt b/file_0.txt"),
            "{}",
            patch.text
        );
        assert!(patch.text.contains("+content 0"), "{}", patch.text);
        assert_eq!(patch.name, format!("{}-add-the-widget.patch", &sha[..7]));
    }

    #[tokio::test]
    async fn the_working_tree_patches_staged_and_unstaged_changes_apart() {
        let repo = TestRepo::new();
        repo.commit("First");
        repo.write("file_0.txt", "changed\n");
        let unstaged = worktree_patch(&cli(), repo.path(), false).await.unwrap();
        assert!(unstaged.text.contains("-content 0"), "{}", unstaged.text);
        assert_eq!(unstaged.name, "changes.patch");

        repo.git(&["add", "file_0.txt"]);
        let staged = worktree_patch(&cli(), repo.path(), true).await.unwrap();
        assert!(staged.text.contains("+changed"), "{}", staged.text);
        assert_eq!(staged.name, "staged-changes.patch");
    }

    #[test]
    fn a_subject_makes_a_short_safe_name() {
        assert_eq!(
            patch_name("0123456789abcdef", "Fix: a/b  c!"),
            "0123456-fix-a-b-c.patch"
        );
        assert_eq!(patch_name("0123456789abcdef", "!!!"), "0123456.patch");
    }
}
