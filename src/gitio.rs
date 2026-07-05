//! Git and gh adapters. Thin wrappers, no decision making.

use std::path::Path;

use crate::procio::{argv, Runner};

fn git(root: &str, args: &[&str]) -> Vec<String> {
    let mut command = argv(&["git", "-C", root]);
    command.extend(args.iter().map(|arg| (*arg).to_owned()));
    command
}

pub fn repo_root(run: &dyn Runner, cwd: &str) -> Option<String> {
    let result = run.run(&git(cwd, &["rev-parse", "--show-toplevel"]), None);
    let text = result.text();
    (result.ok() && !text.is_empty()).then_some(text)
}

pub fn current_branch(run: &dyn Runner, root: &str) -> String {
    let result = run.run(&git(root, &["branch", "--show-current"]), None);
    if result.ok() {
        result.text()
    } else {
        String::new()
    }
}

pub fn default_branch(run: &dyn Runner, root: &str) -> String {
    let result = run.run(
        &git(
            root,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        ),
        None,
    );
    if result.ok() {
        if let Some(branch) = result.text().strip_prefix("origin/") {
            return branch.to_owned();
        }
    }
    default_branch_from_remote(run, root)
}

/// Ask the remote directly when origin/HEAD was never set locally.
fn default_branch_from_remote(run: &dyn Runner, root: &str) -> String {
    let result = run.run(
        &git(root, &["ls-remote", "--symref", "origin", "HEAD"]),
        None,
    );
    if !result.ok() {
        return String::new();
    }
    for line in result.stdout.lines() {
        if let Some(rest) = line.strip_prefix("ref: refs/heads/") {
            if let Some(branch) = rest.split_whitespace().next() {
                return branch.to_owned();
            }
        }
    }
    String::new()
}

pub fn github_origin(run: &dyn Runner, root: &str) -> bool {
    let result = run.run(&git(root, &["remote", "get-url", "origin"]), None);
    result.ok() && result.text().contains("github.com")
}

pub fn linked_worktree(run: &dyn Runner, root: &str) -> bool {
    let result = run.run(
        &git(root, &["rev-parse", "--git-dir", "--git-common-dir"]),
        None,
    );
    if !result.ok() {
        return false;
    }
    let lines: Vec<&str> = result.stdout.lines().collect();
    lines.len() == 2 && lines[0].trim() != lines[1].trim()
}

pub fn dirty(run: &dyn Runner, root: &str) -> bool {
    let result = run.run(&git(root, &["status", "--porcelain"]), None);
    !result.ok() || !result.stdout.trim().is_empty()
}

pub fn tip_sha(run: &dyn Runner, root: &str) -> String {
    let result = run.run(&git(root, &["rev-parse", "HEAD"]), None);
    if result.ok() {
        result.text()
    } else {
        String::new()
    }
}

pub fn last_known_remote_sha(run: &dyn Runner, root: &str, branch: &str) -> String {
    let reference = format!("refs/remotes/origin/{branch}");
    let result = run.run(&git(root, &["rev-parse", &reference]), None);
    if result.ok() {
        result.text()
    } else {
        String::new()
    }
}

/// None means the remote could not be reached; callers must not act on it.
pub fn remote_branch_exists(run: &dyn Runner, root: &str, branch: &str) -> Option<bool> {
    let result = run.run(
        &git(root, &["ls-remote", "--heads", "origin", branch]),
        None,
    );
    result.ok().then(|| !result.stdout.trim().is_empty())
}

pub fn merged_pr_head_sha(run: &dyn Runner, root: &str, branch: &str) -> String {
    let command = argv(&[
        "gh",
        "pr",
        "list",
        "--head",
        branch,
        "--state",
        "merged",
        "--json",
        "headRefOid",
        "--limit",
        "1",
    ]);
    let result = run.run(&command, Some(Path::new(root)));
    if !result.ok() {
        return String::new();
    }
    parse_pr_head(&result.stdout)
}

fn parse_pr_head(payload: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return String::new();
    };
    value
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("headRefOid"))
        .and_then(|sha| sha.as_str())
        .unwrap_or_default()
        .to_owned()
}

/// Switch to the default branch and fast-forward it. None on success.
pub fn checkout_default(run: &dyn Runner, root: &str, default: &str) -> Option<String> {
    let checkout = run.run(&git(root, &["checkout", default]), None);
    if !checkout.ok() {
        return Some(format!("checkout failed: {}", checkout.stderr.trim()));
    }
    let pull = run.run(&git(root, &["pull", "--ff-only"]), None);
    if !pull.ok() {
        return Some(format!(
            "checked out {default}, but pull failed: {}",
            pull.stderr.trim()
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::FakeRunner;

    #[test]
    fn repo_root_returns_toplevel() {
        let run = FakeRunner::new().on("rev-parse --show-toplevel", 0, "/repo\n", "");
        assert_eq!(repo_root(&run, "/repo/sub").as_deref(), Some("/repo"));
    }

    #[test]
    fn repo_root_none_outside_a_repo() {
        let run = FakeRunner::new().on("rev-parse --show-toplevel", 128, "", "");
        assert_eq!(repo_root(&run, "/tmp"), None);
    }

    #[test]
    fn current_branch_name() {
        let run = FakeRunner::new().on("branch --show-current", 0, "feature\n", "");
        assert_eq!(current_branch(&run, "/repo"), "feature");
    }

    #[test]
    fn current_branch_empty_when_detached() {
        let run = FakeRunner::new().on("branch --show-current", 0, "\n", "");
        assert_eq!(current_branch(&run, "/repo"), "");
    }

    #[test]
    fn current_branch_empty_on_error() {
        let run = FakeRunner::new().on("branch --show-current", 128, "", "");
        assert_eq!(current_branch(&run, "/repo"), "");
    }

    #[test]
    fn default_branch_from_local_symbolic_ref() {
        let run = FakeRunner::new().on("symbolic-ref", 0, "origin/main\n", "");
        assert_eq!(default_branch(&run, "/repo"), "main");
    }

    #[test]
    fn default_branch_falls_back_to_remote_symref() {
        let run = FakeRunner::new().on("symbolic-ref", 128, "", "").on(
            "ls-remote --symref",
            0,
            "ref: refs/heads/develop\tHEAD\nabc123\tHEAD\n",
            "",
        );
        assert_eq!(default_branch(&run, "/repo"), "develop");
    }

    #[test]
    fn default_branch_empty_when_remote_unreachable() {
        let run = FakeRunner::new()
            .on("symbolic-ref", 128, "", "")
            .on("ls-remote", 128, "", "");
        assert_eq!(default_branch(&run, "/repo"), "");
    }

    #[test]
    fn default_branch_empty_when_symref_output_malformed() {
        let run = FakeRunner::new().on("symbolic-ref", 128, "", "").on(
            "ls-remote --symref",
            0,
            "abc123\tHEAD\n",
            "",
        );
        assert_eq!(default_branch(&run, "/repo"), "");
    }

    #[test]
    fn github_origin_true_for_github_url() {
        let run = FakeRunner::new().on("remote get-url", 0, "git@github.com:me/repo.git\n", "");
        assert!(github_origin(&run, "/repo"));
    }

    #[test]
    fn github_origin_false_for_other_host() {
        let run = FakeRunner::new().on("remote get-url", 0, "https://gitlab.com/me/repo.git\n", "");
        assert!(!github_origin(&run, "/repo"));
    }

    #[test]
    fn github_origin_false_without_origin() {
        let run = FakeRunner::new().on("remote get-url", 2, "", "");
        assert!(!github_origin(&run, "/repo"));
    }

    #[test]
    fn linked_worktree_false_for_main_checkout() {
        let run = FakeRunner::new().on("--git-dir --git-common-dir", 0, ".git\n.git\n", "");
        assert!(!linked_worktree(&run, "/repo"));
    }

    #[test]
    fn linked_worktree_true_when_dirs_differ() {
        let run = FakeRunner::new().on(
            "--git-dir --git-common-dir",
            0,
            "/repo/.git/worktrees/task\n/repo/.git\n",
            "",
        );
        assert!(linked_worktree(&run, "/wt"));
    }

    #[test]
    fn linked_worktree_error_treated_as_not_linked() {
        let run = FakeRunner::new().on("--git-dir --git-common-dir", 128, "", "");
        assert!(!linked_worktree(&run, "/repo"));
    }

    #[test]
    fn dirty_false_when_clean() {
        let run = FakeRunner::new().on("status --porcelain", 0, "", "");
        assert!(!dirty(&run, "/repo"));
    }

    #[test]
    fn dirty_true_with_changes() {
        let run = FakeRunner::new().on("status --porcelain", 0, " M file.rs\n", "");
        assert!(dirty(&run, "/repo"));
    }

    #[test]
    fn dirty_true_on_error() {
        let run = FakeRunner::new().on("status --porcelain", 128, "", "");
        assert!(dirty(&run, "/repo"));
    }

    #[test]
    fn tip_sha_returned() {
        let run = FakeRunner::new().on("rev-parse HEAD", 0, "abc123\n", "");
        assert_eq!(tip_sha(&run, "/repo"), "abc123");
    }

    #[test]
    fn tip_sha_empty_on_error() {
        let run = FakeRunner::new().on("rev-parse HEAD", 128, "", "");
        assert_eq!(tip_sha(&run, "/repo"), "");
    }

    #[test]
    fn last_known_remote_sha_returned() {
        let run = FakeRunner::new().on("refs/remotes/origin/feature", 0, "def456\n", "");
        assert_eq!(last_known_remote_sha(&run, "/repo", "feature"), "def456");
    }

    #[test]
    fn last_known_remote_sha_empty_when_missing() {
        let run = FakeRunner::new().on("refs/remotes/origin/feature", 128, "", "");
        assert_eq!(last_known_remote_sha(&run, "/repo", "feature"), "");
    }

    #[test]
    fn remote_branch_exists_true() {
        let run = FakeRunner::new().on("ls-remote --heads", 0, "abc\trefs/heads/feature\n", "");
        assert_eq!(remote_branch_exists(&run, "/repo", "feature"), Some(true));
    }

    #[test]
    fn remote_branch_exists_false_when_deleted() {
        let run = FakeRunner::new().on("ls-remote --heads", 0, "", "");
        assert_eq!(remote_branch_exists(&run, "/repo", "feature"), Some(false));
    }

    #[test]
    fn remote_branch_exists_none_when_unreachable() {
        let run = FakeRunner::new().on("ls-remote --heads", 128, "", "");
        assert_eq!(remote_branch_exists(&run, "/repo", "feature"), None);
    }

    #[test]
    fn merged_pr_head_found() {
        let run = FakeRunner::new().on("gh pr list", 0, r#"[{"headRefOid":"abc123"}]"#, "");
        assert_eq!(merged_pr_head_sha(&run, "/repo", "feature"), "abc123");
    }

    #[test]
    fn merged_pr_head_empty_without_pr() {
        let run = FakeRunner::new().on("gh pr list", 0, "[]", "");
        assert_eq!(merged_pr_head_sha(&run, "/repo", "feature"), "");
    }

    #[test]
    fn merged_pr_head_empty_on_gh_failure() {
        let run = FakeRunner::new().on("gh pr list", 4, "", "");
        assert_eq!(merged_pr_head_sha(&run, "/repo", "feature"), "");
    }

    #[test]
    fn merged_pr_head_empty_on_invalid_json() {
        let run = FakeRunner::new().on("gh pr list", 0, "not json", "");
        assert_eq!(merged_pr_head_sha(&run, "/repo", "feature"), "");
    }

    #[test]
    fn merged_pr_head_empty_on_unexpected_shape() {
        let run = FakeRunner::new().on("gh pr list", 0, r#"[{"headRefOid":7}]"#, "");
        assert_eq!(merged_pr_head_sha(&run, "/repo", "feature"), "");
    }

    #[test]
    fn merged_pr_query_runs_in_repo_cwd() {
        let run = FakeRunner::new().on("gh pr list", 0, "[]", "");
        merged_pr_head_sha(&run, "/repo", "feature");
        assert_eq!(run.calls()[0].1.as_deref(), Some("/repo"));
    }

    #[test]
    fn checkout_default_success() {
        let run = FakeRunner::new()
            .on("checkout main", 0, "", "")
            .on("pull --ff-only", 0, "", "");
        assert_eq!(checkout_default(&run, "/repo", "main"), None);
    }

    #[test]
    fn checkout_default_reports_checkout_failure() {
        let run = FakeRunner::new().on("checkout main", 1, "", "conflict");
        assert_eq!(
            checkout_default(&run, "/repo", "main").as_deref(),
            Some("checkout failed: conflict")
        );
    }

    #[test]
    fn checkout_default_reports_pull_failure() {
        let run = FakeRunner::new().on("checkout main", 0, "", "").on(
            "pull --ff-only",
            1,
            "",
            "diverged",
        );
        assert_eq!(
            checkout_default(&run, "/repo", "main").as_deref(),
            Some("checked out main, but pull failed: diverged")
        );
    }
}
