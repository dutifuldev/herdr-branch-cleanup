from herdr_branch_cleanup.core import (
    Action,
    Fate,
    LocalFacts,
    RemoteFacts,
    branch_fate,
    busy_agent_count,
    decide,
    has_unpushed_work,
    hold_reason,
    local_skip_reason,
)


def make_local(**overrides):
    base = dict(
        root="/repo",
        branch="feature",
        default_branch="main",
        github_origin=True,
        linked_worktree=False,
        dirty=False,
        tip_sha="abc123",
        busy_agents=0,
    )
    base.update(overrides)
    return LocalFacts(**base)


def make_remote(**overrides):
    base = dict(
        branch_exists=True,
        merged_pr_head_sha="",
        last_known_remote_sha="abc123",
    )
    base.update(overrides)
    return RemoteFacts(**base)


class TestLocalSkipReason:
    def test_clean_feature_branch_is_not_skipped(self):
        assert local_skip_reason(make_local()) is None

    def test_detached_head(self):
        assert local_skip_reason(make_local(branch="")) == "detached HEAD"

    def test_unknown_default_branch(self):
        assert local_skip_reason(make_local(default_branch="")) == "default branch unknown"

    def test_already_on_default(self):
        reason = local_skip_reason(make_local(branch="main"))
        assert reason == "already on default branch"

    def test_non_github_origin(self):
        assert local_skip_reason(make_local(github_origin=False)) == "origin is not GitHub"

    def test_detached_wins_over_missing_default(self):
        assert local_skip_reason(make_local(branch="", default_branch="")) == "detached HEAD"


class TestBranchFate:
    def test_alive_when_branch_exists_and_no_merged_pr(self):
        assert branch_fate(make_remote()) is Fate.ALIVE

    def test_merged_when_pr_head_known(self):
        assert branch_fate(make_remote(merged_pr_head_sha="abc123")) is Fate.MERGED

    def test_merged_wins_over_deleted(self):
        remote = make_remote(branch_exists=False, merged_pr_head_sha="abc123")
        assert branch_fate(remote) is Fate.MERGED

    def test_deleted_when_branch_gone(self):
        assert branch_fate(make_remote(branch_exists=False)) is Fate.DELETED


class TestHasUnpushedWork:
    def test_merged_and_tip_matches_pr_head(self):
        remote = make_remote(merged_pr_head_sha="abc123")
        assert has_unpushed_work(make_local(), remote, Fate.MERGED) is False

    def test_merged_and_tip_differs_from_pr_head(self):
        remote = make_remote(merged_pr_head_sha="other")
        assert has_unpushed_work(make_local(), remote, Fate.MERGED) is True

    def test_deleted_and_tip_matches_last_known(self):
        remote = make_remote(branch_exists=False)
        assert has_unpushed_work(make_local(), remote, Fate.DELETED) is False

    def test_deleted_and_tip_differs(self):
        remote = make_remote(branch_exists=False, last_known_remote_sha="other")
        assert has_unpushed_work(make_local(), remote, Fate.DELETED) is True

    def test_deleted_with_no_last_known_sha_counts_as_unpushed(self):
        remote = make_remote(branch_exists=False, last_known_remote_sha="")
        assert has_unpushed_work(make_local(), remote, Fate.DELETED) is True


class TestHoldReason:
    def test_no_hold_when_all_gates_pass(self):
        remote = make_remote(merged_pr_head_sha="abc123")
        assert hold_reason(make_local(), remote, Fate.MERGED) is None

    def test_linked_worktree_holds(self):
        remote = make_remote(merged_pr_head_sha="abc123")
        reason = hold_reason(make_local(linked_worktree=True), remote, Fate.MERGED)
        assert reason == "linked worktree, the branch is its identity"

    def test_busy_agents_hold_with_count(self):
        remote = make_remote(merged_pr_head_sha="abc123")
        reason = hold_reason(make_local(busy_agents=2), remote, Fate.MERGED)
        assert reason == "2 agent(s) working in this repo"

    def test_dirty_tree_holds(self):
        remote = make_remote(merged_pr_head_sha="abc123")
        reason = hold_reason(make_local(dirty=True), remote, Fate.MERGED)
        assert reason == "working tree has uncommitted changes"

    def test_unpushed_work_holds(self):
        remote = make_remote(merged_pr_head_sha="other")
        reason = hold_reason(make_local(), remote, Fate.MERGED)
        assert reason == "local tip does not match the last commit GitHub saw"

    def test_worktree_gate_checked_before_busy_agents(self):
        remote = make_remote(merged_pr_head_sha="abc123")
        local = make_local(linked_worktree=True, busy_agents=1)
        assert hold_reason(local, remote, Fate.MERGED) == (
            "linked worktree, the branch is its identity"
        )

    def test_busy_gate_checked_before_dirty(self):
        remote = make_remote(merged_pr_head_sha="abc123")
        local = make_local(busy_agents=1, dirty=True)
        assert hold_reason(local, remote, Fate.MERGED) == "1 agent(s) working in this repo"


class TestDecide:
    def test_skip_when_local_gate_fires(self):
        decision = decide(make_local(branch="main"), make_remote())
        assert decision.action is Action.SKIP
        assert decision.fate is Fate.UNKNOWN
        assert decision.reason == "already on default branch"

    def test_skip_when_branch_alive(self):
        decision = decide(make_local(), make_remote())
        assert decision.action is Action.SKIP
        assert decision.fate is Fate.ALIVE
        assert decision.reason == "branch still exists on the remote"

    def test_hold_when_gate_fires(self):
        decision = decide(make_local(dirty=True), make_remote(merged_pr_head_sha="abc123"))
        assert decision.action is Action.HOLD
        assert decision.fate is Fate.MERGED
        assert decision.reason == "working tree has uncommitted changes"

    def test_checkout_on_clean_merged_branch(self):
        decision = decide(make_local(), make_remote(merged_pr_head_sha="abc123"))
        assert decision.action is Action.CHECKOUT
        assert decision.fate is Fate.MERGED
        assert decision.reason == "branch merged"

    def test_merged_fate_reaches_hold_gates(self):
        # Tip matches the merged PR head but NOT the stale remote ref, so the
        # unpushed gate only passes when hold_reason receives the MERGED fate.
        remote = make_remote(merged_pr_head_sha="abc123", last_known_remote_sha="stale")
        decision = decide(make_local(), remote)
        assert decision.action is Action.CHECKOUT
        assert decision.fate is Fate.MERGED

    def test_checkout_on_clean_deleted_branch(self):
        decision = decide(make_local(), make_remote(branch_exists=False))
        assert decision.action is Action.CHECKOUT
        assert decision.fate is Fate.DELETED
        assert decision.reason == "branch deleted on remote"


class TestBusyAgentCount:
    def test_counts_working_and_blocked_only(self):
        statuses = ["working", "blocked", "idle", "done", "unknown"]
        assert busy_agent_count(statuses) == 2

    def test_empty_list(self):
        assert busy_agent_count([]) == 0

    def test_all_idle(self):
        assert busy_agent_count(["idle", "done"]) == 0
