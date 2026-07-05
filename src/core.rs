//! Pure decision logic: which repos get switched back to the default branch.
//!
//! This module holds no IO. Facts come in, a decision comes out, and every
//! safety gate lives here so it can be tested as a plain function.

/// What happened to the branch on the GitHub remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    Unknown,
    Alive,
    Merged,
    Deleted,
}

impl Fate {
    pub fn as_str(self) -> &'static str {
        match self {
            Fate::Unknown => "unknown",
            Fate::Alive => "alive",
            Fate::Merged => "merged",
            Fate::Deleted => "deleted",
        }
    }
}

/// What the sweeper should do with a repo this cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Checkout,
    Hold,
    Skip,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Checkout => "checkout",
            ActionKind::Hold => "hold",
            ActionKind::Skip => "skip",
        }
    }
}

/// Facts readable from the local repo and the herdr session alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFacts {
    pub root: String,
    pub branch: String,
    pub default_branch: String,
    pub github_origin: bool,
    pub linked_worktree: bool,
    pub dirty: bool,
    pub tip_sha: String,
    pub busy_agents: usize,
}

/// Facts that required talking to GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFacts {
    pub branch_exists: bool,
    pub merged_pr_head_sha: String,
    pub last_known_remote_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub action: ActionKind,
    pub fate: Fate,
    pub reason: String,
}

/// Reasons to leave a repo alone without ever asking the remote.
pub fn local_skip_reason(local: &LocalFacts) -> Option<String> {
    if local.branch.is_empty() {
        return Some("detached HEAD".to_owned());
    }
    if local.default_branch.is_empty() {
        return Some("default branch unknown".to_owned());
    }
    if local.branch == local.default_branch {
        return Some("already on default branch".to_owned());
    }
    if !local.github_origin {
        return Some("origin is not GitHub".to_owned());
    }
    None
}

pub fn branch_fate(remote: &RemoteFacts) -> Fate {
    if !remote.merged_pr_head_sha.is_empty() {
        return Fate::Merged;
    }
    if !remote.branch_exists {
        return Fate::Deleted;
    }
    Fate::Alive
}

/// True when the local tip may hold commits GitHub never saw.
///
/// A squash merge rewrites history, so `git branch --merged` lies; the
/// reliable check is whether the local tip is exactly the commit the remote
/// last knew about (the merged PR head, or the last fetched remote ref).
pub fn has_unpushed_work(local: &LocalFacts, remote: &RemoteFacts, fate: Fate) -> bool {
    if fate == Fate::Merged {
        return local.tip_sha != remote.merged_pr_head_sha;
    }
    remote.last_known_remote_sha.is_empty() || local.tip_sha != remote.last_known_remote_sha
}

/// Safety gates: the branch is gone, but switching now would be unsafe.
pub fn hold_reason(local: &LocalFacts, remote: &RemoteFacts, fate: Fate) -> Option<String> {
    if local.linked_worktree {
        return Some("linked worktree, the branch is its identity".to_owned());
    }
    if local.busy_agents > 0 {
        return Some(format!(
            "{} agent(s) working in this repo",
            local.busy_agents
        ));
    }
    if local.dirty {
        return Some("working tree has uncommitted changes".to_owned());
    }
    if has_unpushed_work(local, remote, fate) {
        return Some("local tip does not match the last commit GitHub saw".to_owned());
    }
    None
}

pub fn decide(local: &LocalFacts, remote: &RemoteFacts) -> Decision {
    if let Some(skip) = local_skip_reason(local) {
        return Decision {
            action: ActionKind::Skip,
            fate: Fate::Unknown,
            reason: skip,
        };
    }
    let fate = branch_fate(remote);
    if fate == Fate::Alive {
        return Decision {
            action: ActionKind::Skip,
            fate,
            reason: "branch still exists on the remote".to_owned(),
        };
    }
    if let Some(hold) = hold_reason(local, remote, fate) {
        return Decision {
            action: ActionKind::Hold,
            fate,
            reason: hold,
        };
    }
    let cause = if fate == Fate::Merged {
        "merged"
    } else {
        "deleted on remote"
    };
    Decision {
        action: ActionKind::Checkout,
        fate,
        reason: format!("branch {cause}"),
    }
}

pub fn busy_agent_count<S: AsRef<str>>(agent_statuses: &[S]) -> usize {
    agent_statuses
        .iter()
        .filter(|status| matches!(status.as_ref(), "working" | "blocked"))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> LocalFacts {
        LocalFacts {
            root: "/repo".to_owned(),
            branch: "feature".to_owned(),
            default_branch: "main".to_owned(),
            github_origin: true,
            linked_worktree: false,
            dirty: false,
            tip_sha: "abc123".to_owned(),
            busy_agents: 0,
        }
    }

    fn remote() -> RemoteFacts {
        RemoteFacts {
            branch_exists: true,
            merged_pr_head_sha: String::new(),
            last_known_remote_sha: "abc123".to_owned(),
        }
    }

    #[test]
    fn clean_feature_branch_is_not_skipped() {
        assert_eq!(local_skip_reason(&local()), None);
    }

    #[test]
    fn detached_head_skips() {
        let facts = LocalFacts {
            branch: String::new(),
            ..local()
        };
        assert_eq!(local_skip_reason(&facts).as_deref(), Some("detached HEAD"));
    }

    #[test]
    fn unknown_default_branch_skips() {
        let facts = LocalFacts {
            default_branch: String::new(),
            ..local()
        };
        assert_eq!(
            local_skip_reason(&facts).as_deref(),
            Some("default branch unknown")
        );
    }

    #[test]
    fn already_on_default_skips() {
        let facts = LocalFacts {
            branch: "main".to_owned(),
            ..local()
        };
        assert_eq!(
            local_skip_reason(&facts).as_deref(),
            Some("already on default branch")
        );
    }

    #[test]
    fn non_github_origin_skips() {
        let facts = LocalFacts {
            github_origin: false,
            ..local()
        };
        assert_eq!(
            local_skip_reason(&facts).as_deref(),
            Some("origin is not GitHub")
        );
    }

    #[test]
    fn detached_wins_over_missing_default() {
        let facts = LocalFacts {
            branch: String::new(),
            default_branch: String::new(),
            ..local()
        };
        assert_eq!(local_skip_reason(&facts).as_deref(), Some("detached HEAD"));
    }

    #[test]
    fn fate_alive_when_branch_exists_and_no_merged_pr() {
        assert_eq!(branch_fate(&remote()), Fate::Alive);
    }

    #[test]
    fn fate_merged_when_pr_head_known() {
        let facts = RemoteFacts {
            merged_pr_head_sha: "abc123".to_owned(),
            ..remote()
        };
        assert_eq!(branch_fate(&facts), Fate::Merged);
    }

    #[test]
    fn fate_merged_wins_over_deleted() {
        let facts = RemoteFacts {
            branch_exists: false,
            merged_pr_head_sha: "abc123".to_owned(),
            ..remote()
        };
        assert_eq!(branch_fate(&facts), Fate::Merged);
    }

    #[test]
    fn fate_deleted_when_branch_gone() {
        let facts = RemoteFacts {
            branch_exists: false,
            ..remote()
        };
        assert_eq!(branch_fate(&facts), Fate::Deleted);
    }

    #[test]
    fn merged_tip_matching_pr_head_is_pushed() {
        let facts = RemoteFacts {
            merged_pr_head_sha: "abc123".to_owned(),
            ..remote()
        };
        assert!(!has_unpushed_work(&local(), &facts, Fate::Merged));
    }

    #[test]
    fn merged_tip_differing_from_pr_head_is_unpushed() {
        let facts = RemoteFacts {
            merged_pr_head_sha: "other".to_owned(),
            ..remote()
        };
        assert!(has_unpushed_work(&local(), &facts, Fate::Merged));
    }

    #[test]
    fn deleted_tip_matching_last_known_is_pushed() {
        let facts = RemoteFacts {
            branch_exists: false,
            ..remote()
        };
        assert!(!has_unpushed_work(&local(), &facts, Fate::Deleted));
    }

    #[test]
    fn deleted_tip_differing_is_unpushed() {
        let facts = RemoteFacts {
            branch_exists: false,
            last_known_remote_sha: "other".to_owned(),
            ..remote()
        };
        assert!(has_unpushed_work(&local(), &facts, Fate::Deleted));
    }

    #[test]
    fn deleted_with_no_last_known_sha_counts_as_unpushed() {
        let facts = RemoteFacts {
            branch_exists: false,
            last_known_remote_sha: String::new(),
            ..remote()
        };
        assert!(has_unpushed_work(&local(), &facts, Fate::Deleted));
    }

    fn merged_remote() -> RemoteFacts {
        RemoteFacts {
            merged_pr_head_sha: "abc123".to_owned(),
            ..remote()
        }
    }

    #[test]
    fn no_hold_when_all_gates_pass() {
        assert_eq!(hold_reason(&local(), &merged_remote(), Fate::Merged), None);
    }

    #[test]
    fn linked_worktree_holds() {
        let facts = LocalFacts {
            linked_worktree: true,
            ..local()
        };
        assert_eq!(
            hold_reason(&facts, &merged_remote(), Fate::Merged).as_deref(),
            Some("linked worktree, the branch is its identity")
        );
    }

    #[test]
    fn busy_agents_hold_with_count() {
        let facts = LocalFacts {
            busy_agents: 2,
            ..local()
        };
        assert_eq!(
            hold_reason(&facts, &merged_remote(), Fate::Merged).as_deref(),
            Some("2 agent(s) working in this repo")
        );
    }

    #[test]
    fn dirty_tree_holds() {
        let facts = LocalFacts {
            dirty: true,
            ..local()
        };
        assert_eq!(
            hold_reason(&facts, &merged_remote(), Fate::Merged).as_deref(),
            Some("working tree has uncommitted changes")
        );
    }

    #[test]
    fn unpushed_work_holds() {
        let facts = RemoteFacts {
            merged_pr_head_sha: "other".to_owned(),
            ..remote()
        };
        assert_eq!(
            hold_reason(&local(), &facts, Fate::Merged).as_deref(),
            Some("local tip does not match the last commit GitHub saw")
        );
    }

    #[test]
    fn worktree_gate_checked_before_busy_agents() {
        let facts = LocalFacts {
            linked_worktree: true,
            busy_agents: 1,
            ..local()
        };
        assert_eq!(
            hold_reason(&facts, &merged_remote(), Fate::Merged).as_deref(),
            Some("linked worktree, the branch is its identity")
        );
    }

    #[test]
    fn busy_gate_checked_before_dirty() {
        let facts = LocalFacts {
            busy_agents: 1,
            dirty: true,
            ..local()
        };
        assert_eq!(
            hold_reason(&facts, &merged_remote(), Fate::Merged).as_deref(),
            Some("1 agent(s) working in this repo")
        );
    }

    #[test]
    fn decide_skips_when_local_gate_fires() {
        let facts = LocalFacts {
            branch: "main".to_owned(),
            ..local()
        };
        let decision = decide(&facts, &remote());
        assert_eq!(decision.action, ActionKind::Skip);
        assert_eq!(decision.fate, Fate::Unknown);
        assert_eq!(decision.reason, "already on default branch");
    }

    #[test]
    fn decide_skips_when_branch_alive() {
        let decision = decide(&local(), &remote());
        assert_eq!(decision.action, ActionKind::Skip);
        assert_eq!(decision.fate, Fate::Alive);
        assert_eq!(decision.reason, "branch still exists on the remote");
    }

    #[test]
    fn decide_holds_when_gate_fires() {
        let facts = LocalFacts {
            dirty: true,
            ..local()
        };
        let decision = decide(&facts, &merged_remote());
        assert_eq!(decision.action, ActionKind::Hold);
        assert_eq!(decision.fate, Fate::Merged);
        assert_eq!(decision.reason, "working tree has uncommitted changes");
    }

    #[test]
    fn decide_checks_out_clean_merged_branch() {
        let decision = decide(&local(), &merged_remote());
        assert_eq!(decision.action, ActionKind::Checkout);
        assert_eq!(decision.fate, Fate::Merged);
        assert_eq!(decision.reason, "branch merged");
    }

    #[test]
    fn decide_checks_out_clean_deleted_branch() {
        let facts = RemoteFacts {
            branch_exists: false,
            ..remote()
        };
        let decision = decide(&local(), &facts);
        assert_eq!(decision.action, ActionKind::Checkout);
        assert_eq!(decision.fate, Fate::Deleted);
        assert_eq!(decision.reason, "branch deleted on remote");
    }

    #[test]
    fn merged_fate_reaches_hold_gates() {
        // Tip matches the merged PR head but NOT the stale remote ref, so the
        // unpushed gate only passes when hold_reason receives the MERGED fate.
        let facts = RemoteFacts {
            merged_pr_head_sha: "abc123".to_owned(),
            last_known_remote_sha: "stale".to_owned(),
            ..remote()
        };
        let decision = decide(&local(), &facts);
        assert_eq!(decision.action, ActionKind::Checkout);
        assert_eq!(decision.fate, Fate::Merged);
    }

    #[test]
    fn busy_agent_count_counts_working_and_blocked_only() {
        let statuses = ["working", "blocked", "idle", "done", "unknown"];
        assert_eq!(busy_agent_count(&statuses), 2);
    }

    #[test]
    fn busy_agent_count_empty() {
        assert_eq!(busy_agent_count::<&str>(&[]), 0);
    }

    #[test]
    fn busy_agent_count_all_idle() {
        assert_eq!(busy_agent_count(&["idle", "done"]), 0);
    }

    #[test]
    fn fate_and_action_names_are_stable() {
        assert_eq!(Fate::Unknown.as_str(), "unknown");
        assert_eq!(Fate::Alive.as_str(), "alive");
        assert_eq!(Fate::Merged.as_str(), "merged");
        assert_eq!(Fate::Deleted.as_str(), "deleted");
        assert_eq!(ActionKind::Checkout.as_str(), "checkout");
        assert_eq!(ActionKind::Hold.as_str(), "hold");
        assert_eq!(ActionKind::Skip.as_str(), "skip");
    }
}
