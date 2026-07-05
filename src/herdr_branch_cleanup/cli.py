"""Command-line entrypoint dispatched by the herdr plugin manifest."""

from __future__ import annotations

import argparse
import os
from dataclasses import asdict
from pathlib import Path

from herdr_branch_cleanup import board, daemon, sweep

TRUTHY = frozenset({"1", "true", "yes", "on"})
COMMANDS = ("run", "once", "start", "stop", "toggle", "status", "board", "nudge")


def config_env(environ: dict[str, str]) -> dict[str, str]:
    """Env vars, with the plugin config file as a lower-priority layer."""
    merged: dict[str, str] = {}
    config_dir = environ.get("HERDR_PLUGIN_CONFIG_DIR")
    if config_dir:
        merged.update(parse_env_file(Path(config_dir) / "config.env"))
    merged.update(environ)
    return merged


def parse_env_file(path: Path) -> dict[str, str]:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError:
        return {}
    values: dict[str, str] = {}
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#") or "=" not in stripped:
            continue
        key, _, value = stripped.partition("=")
        values[key.strip()] = value.strip()
    return values


def settings_from(env: dict[str, str]) -> sweep.Settings:
    return sweep.Settings(
        dry_run=env.get("BRANCH_CLEANUP_DRY_RUN", "").lower() in TRUTHY,
        notify_only=env.get("BRANCH_CLEANUP_NOTIFY_ONLY", "").lower() in TRUTHY,
    )


def interval_from(env: dict[str, str]) -> float:
    raw = env.get("BRANCH_CLEANUP_INTERVAL", "60")
    try:
        return max(5.0, float(raw))
    except ValueError:
        return 60.0


def run_command(command: str, entrypoint: str, env: dict[str, str]) -> int:
    directory = daemon.state_dir()
    settings = settings_from(env)
    if command == "run":
        daemon.run_loop(directory, settings, interval_from(env))
        return 0
    if command == "once":
        for report in sweep.sweep_once(directory, settings):
            print(asdict(report))
        return 0
    if command == "board":
        board.board_loop(directory)
        return 0
    if command == "nudge":
        daemon.nudge(directory)
        return 0
    return run_lifecycle_command(command, directory, entrypoint)


def run_lifecycle_command(command: str, directory: Path, entrypoint: str) -> int:
    if command == "start":
        print(daemon.start(directory, entrypoint))
    elif command == "stop":
        print(daemon.stop(directory))
    elif command == "toggle":
        print(daemon.toggle(directory, entrypoint))
    else:
        print(daemon.status(directory))
    return 0


def main(argv: list[str], entrypoint: str) -> int:
    parser = argparse.ArgumentParser(prog="herdr-branch-cleanup")
    parser.add_argument("command", choices=COMMANDS)
    arguments = parser.parse_args(argv)
    env = config_env(dict(os.environ))
    try:
        return run_command(arguments.command, entrypoint, env)
    except KeyboardInterrupt:
        return 130
