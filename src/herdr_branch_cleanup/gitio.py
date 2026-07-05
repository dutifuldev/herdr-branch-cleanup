"""Git and gh adapters. Thin wrappers, no decision making."""

from __future__ import annotations

import json

from herdr_branch_cleanup import procio


def repo_root(cwd: str, run: procio.Runner = procio.run) -> str | None:
    result = run(["git", "-C", cwd, "rev-parse", "--show-toplevel"])
    return result.text if result.ok and result.text else None


def current_branch(root: str, run: procio.Runner = procio.run) -> str:
    result = run(["git", "-C", root, "branch", "--show-current"])
    return result.text if result.ok else ""


def default_branch(root: str, run: procio.Runner = procio.run) -> str:
    result = run(["git", "-C", root, "symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
    if result.ok and result.text.startswith("origin/"):
        return result.text.removeprefix("origin/")
    return default_branch_from_remote(root, run)


def default_branch_from_remote(root: str, run: procio.Runner) -> str:
    """Ask the remote directly when origin/HEAD was never set locally."""
    result = run(["git", "-C", root, "ls-remote", "--symref", "origin", "HEAD"])
    if not result.ok:
        return ""
    for line in result.stdout.splitlines():
        if line.startswith("ref: refs/heads/"):
            return line.split()[1].removeprefix("refs/heads/")
    return ""


def github_origin(root: str, run: procio.Runner = procio.run) -> bool:
    result = run(["git", "-C", root, "remote", "get-url", "origin"])
    return result.ok and "github.com" in result.text


def linked_worktree(root: str, run: procio.Runner = procio.run) -> bool:
    result = run(["git", "-C", root, "rev-parse", "--git-dir", "--git-common-dir"])
    if not result.ok:
        return False
    lines = result.stdout.splitlines()
    return len(lines) == 2 and lines[0].strip() != lines[1].strip()


def dirty(root: str, run: procio.Runner = procio.run) -> bool:
    result = run(["git", "-C", root, "status", "--porcelain"])
    return bool(result.stdout.strip()) if result.ok else True


def tip_sha(root: str, run: procio.Runner = procio.run) -> str:
    result = run(["git", "-C", root, "rev-parse", "HEAD"])
    return result.text if result.ok else ""


def last_known_remote_sha(root: str, branch: str, run: procio.Runner = procio.run) -> str:
    result = run(["git", "-C", root, "rev-parse", f"refs/remotes/origin/{branch}"])
    return result.text if result.ok else ""


def remote_branch_exists(root: str, branch: str, run: procio.Runner = procio.run) -> bool | None:
    """None means the remote could not be reached; callers must not act on it."""
    result = run(["git", "-C", root, "ls-remote", "--heads", "origin", branch])
    if not result.ok:
        return None
    return bool(result.stdout.strip())


def merged_pr_head_sha(root: str, branch: str, run: procio.Runner = procio.run) -> str:
    argv = [
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
    ]
    result = run(argv, cwd=root)
    if not result.ok:
        return ""
    try:
        rows = json.loads(result.stdout)
    except json.JSONDecodeError:
        return ""
    if isinstance(rows, list) and rows and isinstance(rows[0], dict):
        sha = rows[0].get("headRefOid", "")
        return sha if isinstance(sha, str) else ""
    return ""


def checkout_default(root: str, default: str, run: procio.Runner = procio.run) -> str | None:
    """Switch to the default branch and fast-forward it. None on success."""
    checkout = run(["git", "-C", root, "checkout", default])
    if not checkout.ok:
        return f"checkout failed: {checkout.stderr.strip()}"
    pull = run(["git", "-C", root, "pull", "--ff-only"])
    if not pull.ok:
        return f"checked out {default}, but pull failed: {pull.stderr.strip()}"
    return None
