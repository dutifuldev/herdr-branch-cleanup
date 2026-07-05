"""Herdr CLI adapter: read the live session through HERDR_BIN_PATH."""

from __future__ import annotations

import json
import os
from dataclasses import dataclass

from herdr_branch_cleanup import procio


@dataclass(frozen=True)
class Pane:
    pane_id: str
    cwd: str
    agent_status: str


def herdr_bin() -> str:
    return os.environ.get("HERDR_BIN_PATH", "herdr")


def list_panes(run: procio.Runner = procio.run) -> list[Pane]:
    result = run([herdr_bin(), "pane", "list"])
    if not result.ok:
        return []
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError:
        return []
    rows = payload.get("result", {}).get("panes", []) if isinstance(payload, dict) else []
    panes: list[Pane] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        cwd = row.get("foreground_cwd") or row.get("cwd") or ""
        if not cwd:
            continue
        panes.append(
            Pane(
                pane_id=str(row.get("pane_id", "")),
                cwd=str(cwd),
                agent_status=str(row.get("agent_status", "unknown")),
            )
        )
    return panes
