"""One sweep cycle: gather facts per repo, decide, act, record."""

from __future__ import annotations

import json
import time
from dataclasses import asdict, dataclass
from pathlib import Path

from herdr_branch_cleanup import core, gitio, herdrio, procio

STATUS_FILE = "status.json"


@dataclass(frozen=True)
class RepoReport:
    root: str
    branch: str
    fate: str
    action: str
    reason: str
    checked_out: bool


@dataclass(frozen=True)
class Settings:
    dry_run: bool
    notify_only: bool


def repos_from_panes(
    panes: list[herdrio.Pane], run: procio.Runner = procio.run
) -> dict[str, list[herdrio.Pane]]:
    """Group panes by the git repo root their cwd belongs to."""
    repos: dict[str, list[herdrio.Pane]] = {}
    root_by_cwd: dict[str, str | None] = {}
    for pane in panes:
        if pane.cwd not in root_by_cwd:
            root_by_cwd[pane.cwd] = gitio.repo_root(pane.cwd, run)
        root = root_by_cwd[pane.cwd]
        if root is not None:
            repos.setdefault(root, []).append(pane)
    return repos


def local_facts(
    root: str, panes: list[herdrio.Pane], run: procio.Runner = procio.run
) -> core.LocalFacts:
    return core.LocalFacts(
        root=root,
        branch=gitio.current_branch(root, run),
        default_branch=gitio.default_branch(root, run),
        github_origin=gitio.github_origin(root, run),
        linked_worktree=gitio.linked_worktree(root, run),
        dirty=gitio.dirty(root, run),
        tip_sha=gitio.tip_sha(root, run),
        busy_agents=core.busy_agent_count([pane.agent_status for pane in panes]),
    )


def remote_facts(
    root: str, branch: str, run: procio.Runner = procio.run
) -> core.RemoteFacts | None:
    """None when the remote is unreachable; the repo is skipped this cycle."""
    exists = gitio.remote_branch_exists(root, branch, run)
    if exists is None:
        return None
    return core.RemoteFacts(
        branch_exists=exists,
        merged_pr_head_sha=gitio.merged_pr_head_sha(root, branch, run),
        last_known_remote_sha=gitio.last_known_remote_sha(root, branch, run),
    )


def sweep_repo(
    root: str,
    panes: list[herdrio.Pane],
    settings: Settings,
    run: procio.Runner = procio.run,
) -> RepoReport:
    local = local_facts(root, panes, run)
    skip = core.local_skip_reason(local)
    if skip is not None:
        return report_for(local, core.Decision(core.Action.SKIP, core.Fate.UNKNOWN, skip), False)
    remote = remote_facts(root, local.branch, run)
    if remote is None:
        decision = core.Decision(core.Action.SKIP, core.Fate.UNKNOWN, "remote unreachable")
        return report_for(local, decision, False)
    decision = core.decide(local, remote)
    checked_out = False
    reason = decision.reason
    if decision.action is core.Action.CHECKOUT:
        checked_out, reason = apply_checkout(local, settings, reason, run)
    return report_for(local, decision, checked_out, reason)


def apply_checkout(
    local: core.LocalFacts, settings: Settings, reason: str, run: procio.Runner
) -> tuple[bool, str]:
    if settings.notify_only:
        return False, f"{reason} (notify-only mode, not switching)"
    if settings.dry_run:
        return False, f"{reason} (dry run, would checkout {local.default_branch})"
    error = gitio.checkout_default(local.root, local.default_branch, run)
    if error is not None:
        return False, f"{reason}, but {error}"
    return True, f"{reason}, checked out {local.default_branch}"


def report_for(
    local: core.LocalFacts,
    decision: core.Decision,
    checked_out: bool,
    reason: str | None = None,
) -> RepoReport:
    return RepoReport(
        root=local.root,
        branch=local.branch,
        fate=decision.fate.value,
        action=decision.action.value,
        reason=reason if reason is not None else decision.reason,
        checked_out=checked_out,
    )


def sweep_once(
    state_dir: Path, settings: Settings, run: procio.Runner = procio.run
) -> list[RepoReport]:
    panes = herdrio.list_panes(run)
    reports = [
        sweep_repo(root, repo_panes, settings, run)
        for root, repo_panes in sorted(repos_from_panes(panes, run).items())
    ]
    write_status(state_dir, reports, settings)
    return reports


def write_status(state_dir: Path, reports: list[RepoReport], settings: Settings) -> None:
    state_dir.mkdir(parents=True, exist_ok=True)
    payload = {
        "updated_epoch": time.time(),
        "dry_run": settings.dry_run,
        "notify_only": settings.notify_only,
        "reports": [asdict(report) for report in reports],
    }
    (state_dir / STATUS_FILE).write_text(json.dumps(payload, indent=2), encoding="utf-8")


def read_status(state_dir: Path) -> dict[str, object] | None:
    path = state_dir / STATUS_FILE
    if not path.exists():
        return None
    try:
        loaded = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, OSError):
        return None
    return loaded if isinstance(loaded, dict) else None
