"""Subprocess boundary: the one place that actually spawns processes."""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from typing import Protocol


@dataclass(frozen=True)
class RunResult:
    returncode: int
    stdout: str
    stderr: str

    @property
    def ok(self) -> bool:
        return self.returncode == 0

    @property
    def text(self) -> str:
        return self.stdout.strip()


class Runner(Protocol):
    def __call__(self, argv: list[str], cwd: str | None = None) -> RunResult: ...


def run(argv: list[str], cwd: str | None = None) -> RunResult:
    try:
        completed = subprocess.run(
            argv,
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=60,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return RunResult(returncode=127, stdout="", stderr=str(error))
    return RunResult(
        returncode=completed.returncode,
        stdout=completed.stdout or "",
        stderr=completed.stderr or "",
    )
