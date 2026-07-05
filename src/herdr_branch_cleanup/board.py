"""Render the sweep status as a small text board for the plugin pane."""

from __future__ import annotations

import time
from collections.abc import Callable
from pathlib import Path

from herdr_branch_cleanup import sweep

GLYPHS = {"checkout": "✓", "hold": "⏸", "skip": "·"}


def shorten(path: str, limit: int = 40) -> str:
    if len(path) <= limit:
        return path
    return "…" + path[-(limit - 1) :]


def as_report(row: object) -> dict[str, object] | None:
    if not isinstance(row, dict):
        return None
    return {str(key): value for key, value in row.items()}


def render_report(report: dict[str, object]) -> str:
    action = str(report.get("action", ""))
    glyph = GLYPHS.get(action, "?")
    root = shorten(str(report.get("root", "")))
    branch = str(report.get("branch", "")) or "(detached)"
    reason = str(report.get("reason", ""))
    return f" {glyph} {root}  [{branch}]  {reason}"


def render(status: dict[str, object] | None) -> str:
    lines = ["herdr-branch-cleanup", ""]
    if status is None:
        lines.append(" no sweep has run yet")
        return "\n".join(lines)
    mode = mode_line(status)
    if mode:
        lines.append(mode)
    reports = status.get("reports")
    rows = reports if isinstance(reports, list) else []
    if not rows:
        lines.append(" no git repos in any pane")
    reports_typed = [report for report in map(as_report, rows) if report is not None]
    lines.extend(render_report(report) for report in reports_typed)
    updated = status.get("updated_epoch")
    if isinstance(updated, (int, float)):
        age = max(0, int(time.time() - updated))
        lines.extend(["", f" last sweep {age}s ago · ctrl+c to close"])
    return "\n".join(lines)


def mode_line(status: dict[str, object]) -> str:
    modes = []
    if status.get("dry_run"):
        modes.append("dry run")
    if status.get("notify_only"):
        modes.append("notify only")
    return f" mode: {', '.join(modes)}" if modes else ""


def board_loop(
    state_dir: Path,
    refresh_seconds: float = 2.0,
    max_frames: int | None = None,
    sleep: Callable[[float], None] = time.sleep,
    emit: Callable[[str], None] = print,
) -> None:
    frames = 0
    while max_frames is None or frames < max_frames:
        emit("\x1b[2J\x1b[H" + render(sweep.read_status(state_dir)))
        frames += 1
        if max_frames is not None and frames >= max_frames:
            break
        sleep(refresh_seconds)
