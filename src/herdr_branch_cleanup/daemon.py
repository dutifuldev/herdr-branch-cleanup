"""Background poller lifecycle: pidfile start/stop and the sweep loop."""

from __future__ import annotations

import os
import subprocess
import sys
import time
from collections.abc import Callable
from pathlib import Path
from typing import Protocol

from herdr_branch_cleanup import sweep

PID_FILE = "poller.pid"
LOG_FILE = "poller.log"
NUDGE_FILE = "nudge"


class Spawner(Protocol):
    def __call__(self, argv: list[str], log_path: Path) -> int: ...


def state_dir() -> Path:
    return Path(os.environ.get("HERDR_PLUGIN_STATE_DIR", ".state"))


def read_pid(directory: Path) -> int | None:
    path = directory / PID_FILE
    try:
        return int(path.read_text(encoding="utf-8").strip())
    except (OSError, ValueError):
        return None


def pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except (ProcessLookupError, PermissionError):
        return False
    return True


def running_pid(directory: Path) -> int | None:
    pid = read_pid(directory)
    if pid is not None and pid_alive(pid):
        return pid
    return None


def spawn_detached(argv: list[str], log_path: Path) -> int:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with log_path.open("ab") as log:
        process = subprocess.Popen(
            argv,
            stdout=log,
            stderr=log,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
        )
    return process.pid


def loop_argv(entrypoint: str) -> list[str]:
    return [sys.executable, entrypoint, "run"]


def start(directory: Path, entrypoint: str, spawn: Spawner | None = None) -> str:
    pid = running_pid(directory)
    if pid is not None:
        return f"already running (pid {pid})"
    directory.mkdir(parents=True, exist_ok=True)
    spawner = spawn if spawn is not None else spawn_detached
    new_pid = spawner(loop_argv(entrypoint), directory / LOG_FILE)
    (directory / PID_FILE).write_text(str(new_pid), encoding="utf-8")
    return f"started (pid {new_pid})"


def stop(directory: Path) -> str:
    pid = running_pid(directory)
    if pid is None:
        return "not running"
    try:
        os.kill(pid, 15)
    except OSError as error:
        return f"failed to stop pid {pid}: {error}"
    (directory / PID_FILE).unlink(missing_ok=True)
    return f"stopped (pid {pid})"


def toggle(directory: Path, entrypoint: str, spawn: Spawner | None = None) -> str:
    if running_pid(directory) is not None:
        return stop(directory)
    return start(directory, entrypoint, spawn)


def status(directory: Path) -> str:
    pid = running_pid(directory)
    return f"running (pid {pid})" if pid is not None else "not running"


def nudge(directory: Path) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / NUDGE_FILE).touch()


def nudge_stamp(directory: Path) -> float:
    try:
        return (directory / NUDGE_FILE).stat().st_mtime
    except OSError:
        return 0.0


def wait_for_next_cycle(
    directory: Path,
    interval: float,
    seen_stamp: float,
    sleep: Callable[[float], None] = time.sleep,
) -> float:
    """Sleep up to ``interval`` seconds, waking early when a nudge lands."""
    waited = 0.0
    step = 0.5
    while waited < interval:
        sleep(step)
        waited += step
        stamp = nudge_stamp(directory)
        if stamp > seen_stamp:
            return stamp
    return seen_stamp


def run_loop(
    directory: Path,
    settings: sweep.Settings,
    interval: float,
    max_cycles: int | None = None,
    sleep: Callable[[float], None] = time.sleep,
) -> int:
    """Sweep forever (or ``max_cycles`` times in tests)."""
    (directory / PID_FILE).parent.mkdir(parents=True, exist_ok=True)
    (directory / PID_FILE).write_text(str(os.getpid()), encoding="utf-8")
    stamp = nudge_stamp(directory)
    cycles = 0
    while max_cycles is None or cycles < max_cycles:
        sweep.sweep_once(directory, settings)
        cycles += 1
        if max_cycles is not None and cycles >= max_cycles:
            break
        stamp = wait_for_next_cycle(directory, interval, stamp, sleep)
    return cycles
