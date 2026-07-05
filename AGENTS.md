# herdr-branch-cleanup

Herdr plugin that checks out the default branch when a pane's branch is merged
or deleted on GitHub. Rust, one crate, `serde_json` as the only dependency.
The herdr manifest builds `target/release/herdr-branch-cleanup` via the
plugin's `[[build]]` step and launches that binary for every entrypoint.

Slophammer standards apply; see
https://raw.githubusercontent.com/dutifuldev/slophammer/refs/heads/main/docs/AGENT_ENTRYPOINT.md

## Commands to run before finishing

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo llvm-cov --fail-under-lines 85
```

For changes to `src/core.rs`, also run the mutation gate (scope is pinned in
`.cargo/mutants.toml`; any surviving mutant fails the run):

```sh
cargo mutants
```

## Rules

- `unsafe` is forbidden (`[lints.rust] unsafe_code = "forbid"`); do not add
  crates that require it at this boundary either.
- Cognitive complexity per function is capped at 8
  (`clippy::cognitive_complexity`, threshold in `clippy.toml`).
- No new dependencies without a strong reason; the plugin intentionally builds
  fast from a cold `herdr plugin install`.
- Architecture boundaries: `core.rs` is pure decision logic with no IO and no
  imports from this crate; all subprocess calls go through the `Runner` trait
  in `procio.rs`; `gitio`/`herdrio` are thin adapters; only `sweep.rs`
  orchestrates. Tests inject `testsupport::FakeRunner` instead of spawning
  real processes.
- Every behavior change updates tests. `src/core.rs` changes must keep
  `cargo mutants` fully green.
- Suppressions carry a reason: `#[allow(...)] // why`, never bare.
