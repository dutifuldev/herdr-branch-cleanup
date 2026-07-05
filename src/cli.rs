//! Command dispatch for the plugin entrypoints declared in the manifest.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::board;
use crate::daemon::{self, Spawner};
use crate::procio::Runner;
use crate::sweep::{self, Settings};

pub const COMMANDS: [&str; 8] = [
    "run", "once", "start", "stop", "toggle", "status", "board", "nudge",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    pub state_dir: PathBuf,
    pub settings: Settings,
    pub interval: Duration,
    pub entrypoint: String,
}

/// Env vars, with the plugin config file as a lower-priority layer.
pub fn config_env(environ: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut merged = BTreeMap::new();
    if let Some(config_dir) = environ.get("HERDR_PLUGIN_CONFIG_DIR") {
        merged.extend(parse_env_file(&Path::new(config_dir).join("config.env")));
    }
    merged.extend(environ.clone());
    merged
}

pub fn parse_env_file(path: &Path) -> BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = stripped.split_once('=') {
            values.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    values
}

fn truthy(env: &BTreeMap<String, String>, key: &str) -> bool {
    env.get(key)
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

pub fn settings_from(env: &BTreeMap<String, String>) -> Settings {
    Settings {
        dry_run: truthy(env, "BRANCH_CLEANUP_DRY_RUN"),
        notify_only: truthy(env, "BRANCH_CLEANUP_NOTIFY_ONLY"),
    }
}

pub fn interval_from(env: &BTreeMap<String, String>) -> Duration {
    let seconds = env
        .get("BRANCH_CLEANUP_INTERVAL")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(600.0)
        .max(5.0);
    Duration::from_secs_f64(seconds)
}

pub fn context_from(environ: &BTreeMap<String, String>, entrypoint: String) -> Context {
    let env = config_env(environ);
    let state_dir = env
        .get("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".state"));
    Context {
        state_dir,
        settings: settings_from(&env),
        interval: interval_from(&env),
        entrypoint,
    }
}

pub fn dispatch(
    command: &str,
    ctx: &Context,
    run: &dyn Runner,
    spawner: &dyn Spawner,
    out: &mut dyn FnMut(&str),
) -> i32 {
    match command {
        "run" => {
            daemon::run_loop(
                run,
                &ctx.state_dir,
                ctx.settings,
                ctx.interval,
                None,
                &|step| {
                    std::thread::sleep(step);
                },
            );
            0
        }
        "once" => run_once(ctx, run, out),
        "board" => run_board(ctx, out),
        "nudge" => {
            daemon::nudge(&ctx.state_dir);
            0
        }
        "start" | "stop" | "toggle" | "status" => run_lifecycle(command, ctx, run, spawner, out),
        _ => {
            out(&format!(
                "unknown command; expected one of: {}",
                COMMANDS.join(" ")
            ));
            2
        }
    }
}

fn run_once(ctx: &Context, run: &dyn Runner, out: &mut dyn FnMut(&str)) -> i32 {
    for report in sweep::sweep_once(run, &ctx.state_dir, ctx.settings) {
        out(&format!(
            "{} [{}] {} ({}): {}",
            report.action, report.branch, report.root, report.fate, report.reason
        ));
    }
    0
}

fn run_board(ctx: &Context, out: &mut dyn FnMut(&str)) -> i32 {
    board::board_loop(
        &ctx.state_dir,
        Duration::from_secs(2),
        None,
        &|step| std::thread::sleep(step),
        &mut |frame| out(frame),
    );
    0
}

fn run_lifecycle(
    command: &str,
    ctx: &Context,
    run: &dyn Runner,
    spawner: &dyn Spawner,
    out: &mut dyn FnMut(&str),
) -> i32 {
    let message = match command {
        "start" => daemon::start(run, spawner, &ctx.state_dir, &ctx.entrypoint),
        "stop" => daemon::stop(run, &ctx.state_dir),
        "toggle" => daemon::toggle(run, spawner, &ctx.state_dir, &ctx.entrypoint),
        _ => daemon::status(run, &ctx.state_dir),
    };
    out(&message);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::FakeRunner;

    struct NoSpawner;

    impl Spawner for NoSpawner {
        fn spawn(&self, _command: &[String], _log_path: &Path) -> Result<u32, String> {
            Ok(424242)
        }
    }

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "herdr-branch-cleanup-cli-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn ctx(dir: &Path) -> Context {
        Context {
            state_dir: dir.to_path_buf(),
            settings: Settings {
                dry_run: true,
                notify_only: false,
            },
            interval: Duration::from_secs(60),
            entrypoint: "cleanup-bin".to_owned(),
        }
    }

    #[test]
    fn env_wins_over_config_file() {
        let dir = tempdir("cfg");
        std::fs::write(
            dir.join("config.env"),
            "BRANCH_CLEANUP_INTERVAL=30\nBRANCH_CLEANUP_DRY_RUN=1\n",
        )
        .expect("write");
        let merged = config_env(&env(&[
            ("HERDR_PLUGIN_CONFIG_DIR", dir.to_str().expect("utf8")),
            ("BRANCH_CLEANUP_INTERVAL", "90"),
        ]));
        assert_eq!(merged["BRANCH_CLEANUP_INTERVAL"], "90");
        assert_eq!(merged["BRANCH_CLEANUP_DRY_RUN"], "1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_env_without_config_dir() {
        let merged = config_env(&env(&[("A", "1")]));
        assert_eq!(merged["A"], "1");
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn parse_env_file_skips_comments_and_broken_lines() {
        let dir = tempdir("envfile");
        let path = dir.join("config.env");
        std::fs::write(&path, "# comment\n\nKEY = value\nBROKEN LINE\n").expect("write");
        let values = parse_env_file(&path);
        assert_eq!(values.len(), 1);
        assert_eq!(values["KEY"], "value");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_env_file_missing() {
        assert!(parse_env_file(Path::new("/missing/config.env")).is_empty());
    }

    #[test]
    fn settings_defaults_and_truthy_values() {
        assert_eq!(
            settings_from(&env(&[])),
            Settings {
                dry_run: false,
                notify_only: false
            }
        );
        let parsed = settings_from(&env(&[
            ("BRANCH_CLEANUP_DRY_RUN", "TRUE"),
            ("BRANCH_CLEANUP_NOTIFY_ONLY", "yes"),
        ]));
        assert!(parsed.dry_run);
        assert!(parsed.notify_only);
        assert!(!settings_from(&env(&[("BRANCH_CLEANUP_DRY_RUN", "0")])).dry_run);
    }

    #[test]
    fn interval_default_floor_and_garbage() {
        assert_eq!(interval_from(&env(&[])), Duration::from_secs(600));
        assert_eq!(
            interval_from(&env(&[("BRANCH_CLEANUP_INTERVAL", "120")])),
            Duration::from_secs(120)
        );
        assert_eq!(
            interval_from(&env(&[("BRANCH_CLEANUP_INTERVAL", "1")])),
            Duration::from_secs(5)
        );
        assert_eq!(
            interval_from(&env(&[("BRANCH_CLEANUP_INTERVAL", "soon")])),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn context_reads_state_dir_from_env() {
        let environ = env(&[("HERDR_PLUGIN_STATE_DIR", "/tmp/state")]);
        let ctx = context_from(&environ, "bin".to_owned());
        assert_eq!(ctx.state_dir, PathBuf::from("/tmp/state"));
        let fallback = context_from(&env(&[]), "bin".to_owned());
        assert_eq!(fallback.state_dir, PathBuf::from(".state"));
    }

    #[test]
    fn dispatch_status_command() {
        let dir = tempdir("status");
        let run = FakeRunner::new().on("kill -0", 1, "", "");
        let mut output = String::new();
        let code = dispatch("status", &ctx(&dir), &run, &NoSpawner, &mut |text| {
            output.push_str(text);
        });
        assert_eq!(code, 0);
        assert!(output.contains("not running"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dispatch_start_stop_lifecycle() {
        let dir = tempdir("lifecycle");
        let run = FakeRunner::new().on("kill -0", 1, "", "");
        let mut output = String::new();
        dispatch("start", &ctx(&dir), &run, &NoSpawner, &mut |text| {
            output.push_str(text);
        });
        assert!(output.contains("started (pid 424242)"));
        let alive = FakeRunner::new()
            .on("kill -0", 0, "", "")
            .on("kill -15", 0, "", "");
        output.clear();
        dispatch("stop", &ctx(&dir), &alive, &NoSpawner, &mut |text| {
            output.push_str(text);
        });
        assert!(output.contains("stopped"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dispatch_toggle_command() {
        let dir = tempdir("toggle");
        let run = FakeRunner::new().on("kill -0", 1, "", "");
        let mut output = String::new();
        dispatch("toggle", &ctx(&dir), &run, &NoSpawner, &mut |text| {
            output.push_str(text);
        });
        assert!(output.contains("started"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dispatch_nudge_command() {
        let dir = tempdir("nudge");
        let run = FakeRunner::new();
        let code = dispatch("nudge", &ctx(&dir), &run, &NoSpawner, &mut |_| {});
        assert_eq!(code, 0);
        assert!(daemon::nudge_stamp(&dir) > std::time::SystemTime::UNIX_EPOCH);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dispatch_once_prints_reports() {
        let dir = tempdir("once");
        let run = FakeRunner::new().on("pane list", 0, r#"{"result":{"panes":[]}}"#, "");
        let mut lines = 0;
        let code = dispatch("once", &ctx(&dir), &run, &NoSpawner, &mut |_| {
            lines += 1;
        });
        assert_eq!(code, 0);
        assert_eq!(lines, 0);
        assert!(sweep::read_status(&dir).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dispatch_unknown_command() {
        let dir = tempdir("unknown");
        let run = FakeRunner::new();
        let mut output = String::new();
        let code = dispatch("wat", &ctx(&dir), &run, &NoSpawner, &mut |text| {
            output.push_str(text);
        });
        assert_eq!(code, 2);
        assert!(output.contains("unknown command"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
