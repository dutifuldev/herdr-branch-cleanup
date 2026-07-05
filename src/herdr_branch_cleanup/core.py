"""Pure decision logic: which repos get switched back to the default branch.

This module holds no IO. Facts come in, a decision comes out, and every
safety gate lives here so it can be tested as a plain function.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum

BUSY_AGENT_STATUSES = frozenset({"working", "blocked"})


class Fate(Enum):
    """What happened to the branch on the GitHub remote."""

    UNKNOWN = "unknown"
    ALIVE = "alive"
    MERGED = "merged"
    DELETED = "deleted"


class Action(Enum):
    """What the sweeper should do with a repo this cycle."""

    CHECKOUT = "checkout"
    HOLD = "hold"
    SKIP = "skip"


@dataclass(frozen=True)
class LocalFacts:
    """Facts readable from the local repo and the herdr session alone."""

    root: str
    branch: str
    default_branch: str
    github_origin: bool
    linked_worktree: bool
    dirty: bool
    tip_sha: str
    busy_agents: int


@dataclass(frozen=True)
class RemoteFacts:
    """Facts that required talking to GitHub."""

    branch_exists: bool
    merged_pr_head_sha: str
    last_known_remote_sha: str


@dataclass(frozen=True)
class Decision:
    action: Action
    fate: Fate
    reason: str


def local_skip_reason(local: LocalFacts) -> str | None:
    """Reasons to leave a repo alone without ever asking the remote."""
    if not local.branch:
        return "detached HEAD"
    if not local.default_branch:
        return "default branch unknown"
    if local.branch == local.default_branch:
        return "already on default branch"
    if not local.github_origin:
        return "origin is not GitHub"
    return None


def branch_fate(remote: RemoteFacts) -> Fate:
    if remote.merged_pr_head_sha:
        return Fate.MERGED
    if not remote.branch_exists:
        return Fate.DELETED
    return Fate.ALIVE


def has_unpushed_work(local: LocalFacts, remote: RemoteFacts, fate: Fate) -> bool:
    """True when the local tip may hold commits GitHub never saw.

    A squash merge rewrites history, so ``git branch --merged`` lies; the
    reliable check is whether the local tip is exactly the commit the remote
    last knew about (the merged PR head, or the last fetched remote ref).
    """
    if fate is Fate.MERGED:
        return local.tip_sha != remote.merged_pr_head_sha
    return not remote.last_known_remote_sha or local.tip_sha != remote.last_known_remote_sha


def hold_reason(local: LocalFacts, remote: RemoteFacts, fate: Fate) -> str | None:
    """Safety gates: the branch is gone, but switching now would be unsafe."""
    if local.linked_worktree:
        return "linked worktree, the branch is its identity"
    if local.busy_agents:
        return f"{local.busy_agents} agent(s) working in this repo"
    if local.dirty:
        return "working tree has uncommitted changes"
    if has_unpushed_work(local, remote, fate):
        return "local tip does not match the last commit GitHub saw"
    return None


def decide(local: LocalFacts, remote: RemoteFacts) -> Decision:
    skip = local_skip_reason(local)
    if skip is not None:
        return Decision(Action.SKIP, Fate.UNKNOWN, skip)
    fate = branch_fate(remote)
    if fate is Fate.ALIVE:
        return Decision(Action.SKIP, fate, "branch still exists on the remote")
    hold = hold_reason(local, remote, fate)
    if hold is not None:
        return Decision(Action.HOLD, fate, hold)
    cause = "merged" if fate is Fate.MERGED else "deleted on remote"
    return Decision(Action.CHECKOUT, fate, f"branch {cause}")


def busy_agent_count(agent_statuses: list[str]) -> int:
    return sum(1 for status in agent_statuses if status in BUSY_AGENT_STATUSES)
