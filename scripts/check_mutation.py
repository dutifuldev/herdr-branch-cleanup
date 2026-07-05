#!/usr/bin/env python3
"""Fail below the mutation kill-rate floor.

mutmut exits 0 regardless of survivors, so this gate parses the final run
statistics from a mutmut log and enforces a minimum kill rate: killed plus
timed-out mutants over everything that could have been caught. Skipped and
suspicious mutants stay out of the denominator. Raise the floor as survivors
are triaged; never lower it.

    uv run mutmut run 2>&1 | tee /tmp/mutmut.log
    uv run python scripts/check_mutation.py --min-kill-rate 90 --stats-file /tmp/mutmut.log
"""

from __future__ import annotations

import argparse
import re
import sys

STATS = re.compile(
    r"(?P<done>\d+)/(?P<total>\d+)\s+🎉 (?P<killed>\d+)\s+🫥 (?P<uncovered>\d+)"
    r"\s+⏰ (?P<timeout>\d+)\s+🤔 (?P<suspicious>\d+)\s+🙁 (?P<survived>\d+)"
    r"\s+🔇 (?P<skipped>\d+)"
)


def last_stats(output: str) -> dict[str, int] | None:
    matches = list(STATS.finditer(output))
    if not matches:
        return None
    return {key: int(value) for key, value in matches[-1].groupdict().items()}


def kill_rate(stats: dict[str, int]) -> float:
    caught = stats["killed"] + stats["timeout"]
    catchable = caught + stats["survived"] + stats["uncovered"]
    if catchable == 0:
        return 100.0
    return 100.0 * caught / catchable


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--min-kill-rate", type=float, required=True)
    parser.add_argument("--stats-file", required=True)
    arguments = parser.parse_args()
    with open(arguments.stats_file, encoding="utf-8") as handle:
        output = handle.read()
    stats = last_stats(output)
    if stats is None:
        sys.stderr.write("mutation gate failed: no mutmut statistics found\n")
        return 2
    if stats["done"] != stats["total"]:
        sys.stderr.write(
            f"mutation gate failed: only {stats['done']} of {stats['total']} mutants ran\n"
        )
        return 2
    rate = kill_rate(stats)
    floor = arguments.min_kill_rate
    print(f"mutation kill rate {rate:.1f}% (floor {floor:.1f}%)")
    if rate < floor:
        sys.stderr.write(f"mutation gate failed: {rate:.1f}% is below the {floor:.1f}% floor\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
