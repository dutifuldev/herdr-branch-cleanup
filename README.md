# herdr-branch-cleanup

herdr-branch-cleanup is a [Herdr](https://herdr.dev) plugin that watches the git
branch in every pane's repository. When a branch gets merged or deleted on
GitHub, it checks out the default branch there automatically, so finished agent
work never leaves a pane stranded on a dead branch.

A background poller sweeps every 10 minutes (and immediately after an agent
finishes, which is when a merge most likely just happened). For each repo it
acts only when every safety gate passes:

- no agent in that repo is `working` or `blocked`
- the working tree is clean
- the local tip is exactly the commit GitHub last saw (squash-merge safe: it
  compares against the merged PR's head, not `git branch --merged`)
- the checkout is not a linked git worktree (there the branch is the
  worktree's identity, so the repo is only flagged, never switched)
- the checkout target is the remote's actual default branch (from
  `ls-remote --symref origin HEAD`), never the local `origin/HEAD` guess,
  which can be stale after a default-branch rename

When a gate fails, nothing happens; the repo shows up as held, with the
reason, on the status board.

## Requirements

- Herdr ≥ 0.7.0
- `cargo` (Rust toolchain, for the one-time build on install)
- `git` and `gh` (authenticated: `gh auth status`) on PATH

## Install

```sh
herdr plugin install dutifuldev/herdr-branch-cleanup
```

Herdr runs `cargo build --release` once during install. To pin a
[release tag](https://github.com/dutifuldev/herdr-branch-cleanup/releases)
instead of tracking `main`:

```sh
herdr plugin install dutifuldev/herdr-branch-cleanup --ref <tag>
```

For a local checkout, build it yourself and link:

```sh
cargo build --release
herdr plugin link /path/to/herdr-branch-cleanup
```

The plugin is listed on the [Herdr marketplace](https://herdr.dev/plugins/),
and the binary is also published on
[crates.io](https://crates.io/crates/herdr-branch-cleanup). Note that
`cargo install herdr-branch-cleanup` gives you only the standalone binary —
installing through Herdr is what registers the actions, the board pane, and
the autostart hooks.

The poller starts itself the first time a pane opens in any session. No daemon
setup is needed; it runs as a plain background process managed through the
plugin's actions.

## Usage

The status board shows every tracked repo, its branch, and what the last sweep
decided (`✓` checked out, `⏸` held with the reason, `·` skipped):

```sh
herdr plugin pane open branch-cleanup board
```

Actions, also bindable to keys:

```sh
herdr plugin action invoke branch-cleanup.start
herdr plugin action invoke branch-cleanup.stop
herdr plugin action invoke branch-cleanup.toggle
herdr plugin action invoke branch-cleanup.cleanup-now
```

Example keybinding in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+b"
type = "plugin_action"
command = "branch-cleanup.board"
description = "branch cleanup board"
```

## Configuration

Optional, via `config.env` in the plugin config dir (print its path with
`herdr plugin config-dir branch-cleanup`):

```sh
BRANCH_CLEANUP_INTERVAL=600     # seconds between sweeps (default 10 min, min 5)
BRANCH_CLEANUP_DRY_RUN=1        # log what would happen, never switch branches
BRANCH_CLEANUP_NOTIFY_ONLY=1    # detect and show on the board, never switch
```

## License

[MIT](LICENSE)
