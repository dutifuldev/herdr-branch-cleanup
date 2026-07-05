# herdr-branch-cleanup

Herdr plugin that checks out the default branch when a pane's branch is merged
or deleted on GitHub. Zero runtime dependencies: the plugin runs from
`scripts/run.py` with the system `python3`; the dev toolchain uses `uv`.

Slophammer standards apply; see
https://raw.githubusercontent.com/dutifuldev/slophammer/refs/heads/main/docs/AGENT_ENTRYPOINT.md

## Commands to run before finishing

```sh
uv sync
uv run ruff format --check .
uv run ruff check .
uv run ty check src
uv run pytest --cov=src/herdr_branch_cleanup --cov-fail-under=85
```

For changes to `src/herdr_branch_cleanup/core.py`, also run the mutation gate:

```sh
uv run mutmut run 2>&1 | tee /tmp/mutmut.log
uv run python scripts/check_mutation.py --min-kill-rate 90 --stats-file /tmp/mutmut.log
```

## Rules

- Strict typing: every signature annotated; no `Any` (`ANN401` is on). Escapes
  need `# noqa: ANN401 -- reason` with a real reason.
- Suppressions carry a reason: `# noqa: CODE -- why`, never bare.
- No new dependencies. The runtime is stdlib-only by design so
  `herdr plugin install` needs no build step; keep it that way.
- Architecture boundaries (enforced by `slophammer.yml`): `core.py` is pure
  decision logic with no IO and imports nothing from this package; all
  subprocess calls go through `procio.py`; `gitio`/`herdrio` are thin adapters;
  only `sweep.py` orchestrates. Tests inject `FakeRunner` from
  `tests/conftest.py` instead of monkeypatching subprocess.
- Every behavior change updates tests. `core.py` decision changes must keep
  the mutation kill rate at or above the CI floor.
- `scripts/run.py` must keep working on a bare `python3` invocation with the
  plugin directory as cwd; it is the entrypoint the herdr manifest launches.
