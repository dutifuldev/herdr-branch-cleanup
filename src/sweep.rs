//! One sweep cycle: gather facts per repo, decide, act, record.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::core::{self, ActionKind, Decision, Fate, LocalFacts, RemoteFacts};
use crate::gitio;
use crate::herdrio::{self, Pane};
use crate::procio::Runner;

pub const STATUS_FILE: &str = "status.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoReport {
    pub root: String,
    pub branch: String,
    pub fate: &'static str,
    pub action: &'static str,
    pub reason: String,
    pub checked_out: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub dry_run: bool,
    pub notify_only: bool,
}

/// Group panes by the git repo root their cwd belongs to.
pub fn repos_from_panes(run: &dyn Runner, panes: &[Pane]) -> BTreeMap<String, Vec<Pane>> {
    let mut repos: BTreeMap<String, Vec<Pane>> = BTreeMap::new();
    let mut root_by_cwd: BTreeMap<String, Option<String>> = BTreeMap::new();
    for pane in panes {
        let root = root_by_cwd
            .entry(pane.cwd.clone())
            .or_insert_with(|| gitio::repo_root(run, &pane.cwd));
        if let Some(root) = root {
            repos.entry(root.clone()).or_default().push(pane.clone());
        }
    }
    repos
}

pub fn local_facts(run: &dyn Runner, root: &str, panes: &[Pane]) -> LocalFacts {
    let statuses: Vec<&str> = panes
        .iter()
        .map(|pane| pane.agent_status.as_str())
        .collect();
    LocalFacts {
        root: root.to_owned(),
        branch: gitio::current_branch(run, root),
        default_branch: gitio::default_branch(run, root),
        github_origin: gitio::github_origin(run, root),
        linked_worktree: gitio::linked_worktree(run, root),
        dirty: gitio::dirty(run, root),
        tip_sha: gitio::tip_sha(run, root),
        busy_agents: core::busy_agent_count(&statuses),
    }
}

/// None when the remote is unreachable; the repo is skipped this cycle.
pub fn remote_facts(run: &dyn Runner, root: &str, branch: &str) -> Option<RemoteFacts> {
    let probe = gitio::remote_probe(run, root, branch)?;
    Some(RemoteFacts {
        branch_exists: probe.branch_exists,
        remote_default_branch: probe.default_branch,
        merged_pr_head_sha: gitio::merged_pr_head_sha(run, root, branch),
        last_known_remote_sha: gitio::last_known_remote_sha(run, root, branch),
    })
}

pub fn sweep_repo(run: &dyn Runner, root: &str, panes: &[Pane], settings: Settings) -> RepoReport {
    let local = local_facts(run, root, panes);
    if let Some(skip) = core::local_skip_reason(&local) {
        return report_for(&local, ActionKind::Skip, Fate::Unknown, skip, false);
    }
    let Some(remote) = remote_facts(run, &local.root, &local.branch) else {
        let reason = "remote unreachable".to_owned();
        return report_for(&local, ActionKind::Skip, Fate::Unknown, reason, false);
    };
    let decision = core::decide(&local, &remote);
    // The checkout target is the remote's authoritative default branch; the
    // local origin/HEAD guess in `local.default_branch` can be stale.
    finish_report(
        run,
        &local,
        &remote.remote_default_branch,
        &decision,
        settings,
    )
}

fn finish_report(
    run: &dyn Runner,
    local: &LocalFacts,
    target_branch: &str,
    decision: &Decision,
    settings: Settings,
) -> RepoReport {
    if decision.action != ActionKind::Checkout {
        return report_for(
            local,
            decision.action,
            decision.fate,
            decision.reason.clone(),
            false,
        );
    }
    let (checked_out, reason) =
        apply_checkout(run, local, target_branch, settings, &decision.reason);
    report_for(local, decision.action, decision.fate, reason, checked_out)
}

fn apply_checkout(
    run: &dyn Runner,
    local: &LocalFacts,
    target_branch: &str,
    settings: Settings,
    reason: &str,
) -> (bool, String) {
    if settings.notify_only {
        return (false, format!("{reason} (notify-only mode, not switching)"));
    }
    if settings.dry_run {
        return (
            false,
            format!("{reason} (dry run, would checkout {target_branch})"),
        );
    }
    match gitio::checkout_default(run, &local.root, target_branch) {
        Some(error) => (false, format!("{reason}, but {error}")),
        None => (true, format!("{reason}, checked out {target_branch}")),
    }
}

fn report_for(
    local: &LocalFacts,
    action: ActionKind,
    fate: Fate,
    reason: String,
    checked_out: bool,
) -> RepoReport {
    RepoReport {
        root: local.root.clone(),
        branch: local.branch.clone(),
        fate: fate.as_str(),
        action: action.as_str(),
        reason,
        checked_out,
    }
}

pub fn sweep_once(run: &dyn Runner, state_dir: &Path, settings: Settings) -> Vec<RepoReport> {
    let panes = herdrio::list_panes(run);
    let reports: Vec<RepoReport> = repos_from_panes(run, &panes)
        .iter()
        .map(|(root, repo_panes)| sweep_repo(run, root, repo_panes, settings))
        .collect();
    write_status(state_dir, &reports, settings);
    reports
}

fn epoch_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn write_status(state_dir: &Path, reports: &[RepoReport], settings: Settings) {
    let _ = std::fs::create_dir_all(state_dir);
    let rows: Vec<serde_json::Value> = reports
        .iter()
        .map(|report| {
            json!({
                "root": report.root,
                "branch": report.branch,
                "fate": report.fate,
                "action": report.action,
                "reason": report.reason,
                "checked_out": report.checked_out,
            })
        })
        .collect();
    let payload = json!({
        "updated_epoch": epoch_seconds(),
        "dry_run": settings.dry_run,
        "notify_only": settings.notify_only,
        "reports": rows,
    });
    let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
    let _ = std::fs::write(state_dir.join(STATUS_FILE), text);
}

pub fn read_status(state_dir: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(state_dir.join(STATUS_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.is_object().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::FakeRunner;

    const AUTO: Settings = Settings {
        dry_run: false,
        notify_only: false,
    };
    const DRY: Settings = Settings {
        dry_run: true,
        notify_only: false,
    };
    const NOTIFY: Settings = Settings {
        dry_run: false,
        notify_only: true,
    };

    /// A repo on a clean feature branch whose PR was merged on GitHub.
    fn merged_branch_runner() -> FakeRunner {
        FakeRunner::new()
            .on("rev-parse --show-toplevel", 0, "/repo\n", "")
            .on("branch --show-current", 0, "feature\n", "")
            .on("symbolic-ref", 0, "origin/main\n", "")
            .on("remote get-url", 0, "git@github.com:me/repo.git\n", "")
            .on("--git-dir --git-common-dir", 0, ".git\n.git\n", "")
            .on("status --porcelain", 0, "", "")
            .on("rev-parse HEAD", 0, "abc123\n", "")
            .on(
                "ls-remote --symref",
                0,
                "ref: refs/heads/main\tHEAD\nef5b2e9\tHEAD\n",
                "",
            )
            .on("gh pr list", 0, r#"[{"headRefOid":"abc123"}]"#, "")
            .on("refs/remotes/origin/feature", 0, "abc123\n", "")
            .on("checkout main", 0, "", "")
            .on("pull --ff-only", 0, "", "")
    }

    fn pane(cwd: &str, status: &str, id: &str) -> Pane {
        Pane {
            pane_id: id.to_owned(),
            cwd: cwd.to_owned(),
            agent_status: status.to_owned(),
        }
    }

    fn called(run: &FakeRunner, needle: &str) -> bool {
        run.calls()
            .iter()
            .any(|(argv, _)| argv.join(" ").contains(needle))
    }

    #[test]
    fn groups_panes_by_repo_root() {
        let run = FakeRunner::new().on("rev-parse --show-toplevel", 0, "/repo\n", "");
        let panes = [
            pane("/repo", "idle", "w1-1"),
            pane("/repo/sub", "idle", "w1-2"),
        ];
        let repos = repos_from_panes(&run, &panes);
        assert_eq!(repos.keys().collect::<Vec<_>>(), vec!["/repo"]);
        assert_eq!(repos["/repo"].len(), 2);
    }

    #[test]
    fn non_repo_panes_dropped() {
        let run = FakeRunner::new().on("rev-parse --show-toplevel", 128, "", "");
        assert!(repos_from_panes(&run, &[pane("/tmp", "idle", "w1-1")]).is_empty());
    }

    #[test]
    fn repo_root_resolved_once_per_cwd() {
        let run = FakeRunner::new().on("rev-parse --show-toplevel", 0, "/repo\n", "");
        repos_from_panes(
            &run,
            &[pane("/repo", "idle", "a"), pane("/repo", "idle", "b")],
        );
        assert_eq!(run.calls().len(), 1);
    }

    #[test]
    fn checks_out_merged_branch() {
        let run = merged_branch_runner();
        let report = sweep_repo(&run, "/repo", &[pane("/repo", "idle", "w1-1")], AUTO);
        assert_eq!(report.action, "checkout");
        assert!(report.checked_out);
        assert!(report.reason.contains("checked out main"));
        assert!(called(&run, "checkout main"));
    }

    #[test]
    fn dry_run_reports_without_acting() {
        let run = merged_branch_runner();
        let report = sweep_repo(&run, "/repo", &[pane("/repo", "idle", "w1-1")], DRY);
        assert!(!report.checked_out);
        assert!(report.reason.contains("dry run"));
        assert!(!called(&run, "checkout main"));
    }

    #[test]
    fn notify_only_reports_without_acting() {
        let run = merged_branch_runner();
        let report = sweep_repo(&run, "/repo", &[pane("/repo", "idle", "w1-1")], NOTIFY);
        assert!(!report.checked_out);
        assert!(report.reason.contains("notify-only"));
        assert!(!called(&run, "checkout main"));
    }

    #[test]
    fn local_skip_short_circuits_remote_checks() {
        let run = FakeRunner::new()
            .on("branch --show-current", 0, "main\n", "")
            .on("symbolic-ref", 0, "origin/main\n", "")
            .on("remote get-url", 0, "git@github.com:me/repo.git\n", "")
            .on("--git-dir --git-common-dir", 0, ".git\n.git\n", "")
            .on("status --porcelain", 0, "", "")
            .on("rev-parse HEAD", 0, "abc123\n", "");
        let report = sweep_repo(&run, "/repo", &[pane("/repo", "idle", "w1-1")], AUTO);
        assert_eq!(report.action, "skip");
        assert_eq!(report.reason, "already on default branch");
        assert!(!called(&run, "ls-remote"));
    }

    #[test]
    fn unreachable_remote_skips() {
        let mut run = merged_branch_runner();
        run.drop_response("ls-remote --symref");
        let run = run.on("ls-remote --symref", 128, "", "");
        let report = sweep_repo(&run, "/repo", &[pane("/repo", "idle", "w1-1")], AUTO);
        assert_eq!(report.action, "skip");
        assert_eq!(report.reason, "remote unreachable");
    }

    #[test]
    fn busy_agent_holds() {
        let run = merged_branch_runner();
        let report = sweep_repo(&run, "/repo", &[pane("/repo", "working", "w1-1")], AUTO);
        assert_eq!(report.action, "hold");
        assert!(!report.checked_out);
    }

    #[test]
    fn stale_local_origin_head_never_picks_checkout_target() {
        // Regression: clones carried origin/HEAD pointing at a non-default
        // branch, and the sweep checked out that wrong branch. The target
        // must come from the remote probe, not the local symbolic ref.
        let mut run = merged_branch_runner();
        run.drop_response("symbolic-ref");
        run.drop_response("checkout main");
        let run =
            run.on("symbolic-ref", 0, "origin/feature-b\n", "")
                .on("checkout main", 0, "", "");
        let report = sweep_repo(&run, "/repo", &[pane("/repo", "idle", "w1-1")], AUTO);
        assert_eq!(report.action, "checkout");
        assert!(report.checked_out);
        assert!(report.reason.contains("checked out main"));
        assert!(called(&run, "checkout main"));
        assert!(!called(&run, "checkout feature-b"));
    }

    #[test]
    fn checkout_failure_reported() {
        let mut run = merged_branch_runner();
        run.drop_response("checkout main");
        let run = run.on("checkout main", 1, "", "conflict");
        let report = sweep_repo(&run, "/repo", &[pane("/repo", "idle", "w1-1")], AUTO);
        assert!(!report.checked_out);
        assert!(report.reason.contains("checkout failed"));
    }

    #[test]
    fn sweep_once_writes_status_file() {
        let dir = tempdir("sweep-status");
        let body = r#"{"id":"x","result":{"panes":[{"pane_id":"w1-1","cwd":"/repo"}]}}"#;
        let run = merged_branch_runner().on("pane list", 0, body, "");
        let reports = sweep_once(&run, &dir, DRY);
        assert_eq!(reports.len(), 1);
        let status = read_status(&dir).expect("status written");
        assert_eq!(status["dry_run"], serde_json::Value::Bool(true));
        assert_eq!(status["reports"][0]["branch"], "feature");
        cleanup(&dir);
    }

    #[test]
    fn sweep_once_with_no_panes_writes_empty_status() {
        let dir = tempdir("sweep-empty");
        let run = FakeRunner::new().on("pane list", 0, r#"{"result":{"panes":[]}}"#, "");
        assert!(sweep_once(&run, &dir, AUTO).is_empty());
        let status = read_status(&dir).expect("status written");
        assert!(status["reports"].as_array().expect("array").is_empty());
        cleanup(&dir);
    }

    #[test]
    fn read_status_missing_file() {
        assert_eq!(read_status(Path::new("/definitely/missing/dir")), None);
    }

    #[test]
    fn read_status_corrupt_file() {
        let dir = tempdir("sweep-corrupt");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join(STATUS_FILE), "not json").expect("write");
        assert_eq!(read_status(&dir), None);
        cleanup(&dir);
    }

    #[test]
    fn read_status_non_object_payload() {
        let dir = tempdir("sweep-nonobject");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join(STATUS_FILE), "[]").expect("write");
        assert_eq!(read_status(&dir), None);
        cleanup(&dir);
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "herdr-branch-cleanup-test-{tag}-{}",
            std::process::id()
        ))
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }
}
