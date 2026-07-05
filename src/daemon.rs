//! Background poller lifecycle: pidfile start/stop and the sweep loop.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::procio::{argv, Runner};
use crate::sweep::{self, Settings};

pub const PID_FILE: &str = "poller.pid";
pub const LOG_FILE: &str = "poller.log";
pub const NUDGE_FILE: &str = "nudge";

pub fn state_dir() -> PathBuf {
    std::env::var("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".state"))
}

pub trait Spawner {
    fn spawn(&self, command: &[String], log_path: &Path) -> Result<u32, String>;
}

pub struct SystemSpawner;

impl Spawner for SystemSpawner {
    fn spawn(&self, command: &[String], log_path: &Path) -> Result<u32, String> {
        let Some((program, args)) = command.split_first() else {
            return Err("empty spawn command".to_owned());
        };
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(|error| error.to_string())?;
        let log_err = log.try_clone().map_err(|error| error.to_string())?;
        spawn_detached(program, args, log, log_err)
    }
}

#[cfg(unix)]
fn spawn_detached(
    program: &str,
    args: &[String],
    log: std::fs::File,
    log_err: std::fs::File,
) -> Result<u32, String> {
    use std::os::unix::process::CommandExt;
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err)
        .process_group(0)
        .spawn()
        .map(|child| child.id())
        .map_err(|error| error.to_string())
}

pub fn read_pid(directory: &Path) -> Option<u32> {
    std::fs::read_to_string(directory.join(PID_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn pid_alive(run: &dyn Runner, pid: u32) -> bool {
    run.run(&argv(&["kill", "-0", &pid.to_string()]), None).ok()
}

pub fn running_pid(run: &dyn Runner, directory: &Path) -> Option<u32> {
    read_pid(directory).filter(|pid| pid_alive(run, *pid))
}

pub fn loop_command(entrypoint: &str) -> Vec<String> {
    vec![entrypoint.to_owned(), "run".to_owned()]
}

pub fn start(
    run: &dyn Runner,
    spawner: &dyn Spawner,
    directory: &Path,
    entrypoint: &str,
) -> String {
    if let Some(pid) = running_pid(run, directory) {
        return format!("already running (pid {pid})");
    }
    let _ = std::fs::create_dir_all(directory);
    match spawner.spawn(&loop_command(entrypoint), &directory.join(LOG_FILE)) {
        Ok(pid) => {
            let _ = std::fs::write(directory.join(PID_FILE), pid.to_string());
            format!("started (pid {pid})")
        }
        Err(error) => format!("failed to start: {error}"),
    }
}

pub fn stop(run: &dyn Runner, directory: &Path) -> String {
    let Some(pid) = running_pid(run, directory) else {
        return "not running".to_owned();
    };
    let result = run.run(&argv(&["kill", "-15", &pid.to_string()]), None);
    if !result.ok() {
        return format!("failed to stop pid {pid}: {}", result.stderr.trim());
    }
    let _ = std::fs::remove_file(directory.join(PID_FILE));
    format!("stopped (pid {pid})")
}

pub fn toggle(
    run: &dyn Runner,
    spawner: &dyn Spawner,
    directory: &Path,
    entrypoint: &str,
) -> String {
    if running_pid(run, directory).is_some() {
        stop(run, directory)
    } else {
        start(run, spawner, directory, entrypoint)
    }
}

pub fn status(run: &dyn Runner, directory: &Path) -> String {
    match running_pid(run, directory) {
        Some(pid) => format!("running (pid {pid})"),
        None => "not running".to_owned(),
    }
}

pub fn nudge(directory: &Path) {
    let _ = std::fs::create_dir_all(directory);
    let _ = std::fs::write(directory.join(NUDGE_FILE), b"nudge");
}

pub fn nudge_stamp(directory: &Path) -> SystemTime {
    std::fs::metadata(directory.join(NUDGE_FILE))
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Sleep up to `interval`, waking early when a nudge lands.
pub fn wait_for_next_cycle(
    directory: &Path,
    interval: Duration,
    seen_stamp: SystemTime,
    sleep: &dyn Fn(Duration),
) -> SystemTime {
    let step = Duration::from_millis(500);
    let mut waited = Duration::ZERO;
    while waited < interval {
        sleep(step);
        waited += step;
        let stamp = nudge_stamp(directory);
        if stamp > seen_stamp {
            return stamp;
        }
    }
    seen_stamp
}

/// Sweep forever (or `max_cycles` times in tests).
pub fn run_loop(
    run: &dyn Runner,
    directory: &Path,
    settings: Settings,
    interval: Duration,
    max_cycles: Option<u64>,
    sleep: &dyn Fn(Duration),
) -> u64 {
    let _ = std::fs::create_dir_all(directory);
    let _ = std::fs::write(directory.join(PID_FILE), std::process::id().to_string());
    let mut stamp = nudge_stamp(directory);
    let mut cycles = 0;
    loop {
        sweep::sweep_once(run, directory, settings);
        cycles += 1;
        if max_cycles.is_some_and(|max| cycles >= max) {
            return cycles;
        }
        stamp = wait_for_next_cycle(directory, interval, stamp, sleep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::FakeRunner;
    use std::cell::RefCell;

    struct FakeSpawner {
        pid: u32,
        calls: RefCell<Vec<(Vec<String>, PathBuf)>>,
        fail: bool,
    }

    impl FakeSpawner {
        fn new(pid: u32) -> Self {
            FakeSpawner {
                pid,
                calls: RefCell::new(Vec::new()),
                fail: false,
            }
        }
    }

    impl Spawner for FakeSpawner {
        fn spawn(&self, command: &[String], log_path: &Path) -> Result<u32, String> {
            self.calls
                .borrow_mut()
                .push((command.to_vec(), log_path.to_path_buf()));
            if self.fail {
                Err("spawn refused".to_owned())
            } else {
                Ok(self.pid)
            }
        }
    }

    fn alive_runner() -> FakeRunner {
        FakeRunner::new().on("kill -0", 0, "", "")
    }

    fn dead_runner() -> FakeRunner {
        FakeRunner::new()
            .on("kill -0", 1, "", "")
            .on("kill -15", 0, "", "")
    }

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "herdr-branch-cleanup-daemon-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn read_pid_missing_and_garbage() {
        let dir = tempdir("pid");
        assert_eq!(read_pid(&dir), None);
        std::fs::write(dir.join(PID_FILE), "nope").expect("write");
        assert_eq!(read_pid(&dir), None);
        cleanup(&dir);
    }

    #[test]
    fn running_pid_for_live_process() {
        let dir = tempdir("live");
        std::fs::write(dir.join(PID_FILE), "4321").expect("write");
        assert_eq!(running_pid(&alive_runner(), &dir), Some(4321));
        cleanup(&dir);
    }

    #[test]
    fn running_pid_for_dead_process() {
        let dir = tempdir("dead");
        std::fs::write(dir.join(PID_FILE), "4321").expect("write");
        assert_eq!(running_pid(&dead_runner(), &dir), None);
        cleanup(&dir);
    }

    #[test]
    fn start_spawns_and_writes_pidfile() {
        let dir = tempdir("start");
        let spawner = FakeSpawner::new(4321);
        let message = start(&dead_runner(), &spawner, &dir, "/plugin/bin/cleanup");
        assert_eq!(message, "started (pid 4321)");
        assert_eq!(read_pid(&dir), Some(4321));
        let calls = spawner.calls.borrow();
        assert_eq!(calls[0].0, vec!["/plugin/bin/cleanup", "run"]);
        assert_eq!(calls[0].1, dir.join(LOG_FILE));
        cleanup(&dir);
    }

    #[test]
    fn start_is_idempotent_when_running() {
        let dir = tempdir("idem");
        std::fs::write(dir.join(PID_FILE), "77").expect("write");
        let spawner = FakeSpawner::new(4321);
        let message = start(&alive_runner(), &spawner, &dir, "bin");
        assert!(message.starts_with("already running"));
        assert!(spawner.calls.borrow().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn start_reports_spawn_failure() {
        let dir = tempdir("fail");
        let mut spawner = FakeSpawner::new(0);
        spawner.fail = true;
        let message = start(&dead_runner(), &spawner, &dir, "bin");
        assert!(message.contains("failed to start"));
        cleanup(&dir);
    }

    #[test]
    fn stop_when_not_running() {
        let dir = tempdir("stopnone");
        assert_eq!(stop(&dead_runner(), &dir), "not running");
        cleanup(&dir);
    }

    #[test]
    fn stop_kills_and_clears_pidfile() {
        let dir = tempdir("stop");
        std::fs::write(dir.join(PID_FILE), "4321").expect("write");
        let run = FakeRunner::new()
            .on("kill -0", 0, "", "")
            .on("kill -15", 0, "", "");
        assert_eq!(stop(&run, &dir), "stopped (pid 4321)");
        assert_eq!(read_pid(&dir), None);
        cleanup(&dir);
    }

    #[test]
    fn toggle_starts_when_stopped() {
        let dir = tempdir("toggle");
        let spawner = FakeSpawner::new(9);
        let message = toggle(&dead_runner(), &spawner, &dir, "bin");
        assert!(message.starts_with("started"));
        cleanup(&dir);
    }

    #[test]
    fn status_reports_both_states() {
        let dir = tempdir("status");
        assert_eq!(status(&dead_runner(), &dir), "not running");
        std::fs::write(dir.join(PID_FILE), "4321").expect("write");
        assert_eq!(status(&alive_runner(), &dir), "running (pid 4321)");
        cleanup(&dir);
    }

    #[test]
    fn nudge_touches_marker() {
        let dir = tempdir("nudge");
        assert_eq!(nudge_stamp(&dir), SystemTime::UNIX_EPOCH);
        nudge(&dir);
        assert!(nudge_stamp(&dir) > SystemTime::UNIX_EPOCH);
        cleanup(&dir);
    }

    #[test]
    fn wait_wakes_early_on_nudge() {
        let dir = tempdir("wake");
        let slept = RefCell::new(0u32);
        let dir_for_sleep = dir.clone();
        let sleep = move |_: Duration| {
            *slept.borrow_mut() += 1;
            nudge(&dir_for_sleep);
        };
        let stamp = wait_for_next_cycle(
            &dir,
            Duration::from_secs(60),
            SystemTime::UNIX_EPOCH,
            &sleep,
        );
        assert!(stamp > SystemTime::UNIX_EPOCH);
        cleanup(&dir);
    }

    #[test]
    fn wait_runs_full_interval_without_nudge() {
        let dir = tempdir("full");
        let stamp = wait_for_next_cycle(
            &dir,
            Duration::from_secs(2),
            SystemTime::UNIX_EPOCH,
            &|_| {},
        );
        assert_eq!(stamp, SystemTime::UNIX_EPOCH);
        cleanup(&dir);
    }

    #[test]
    fn run_loop_runs_requested_cycles() {
        let dir = tempdir("loop");
        let run = FakeRunner::new().on("pane list", 0, r#"{"result":{"panes":[]}}"#, "");
        let settings = Settings {
            dry_run: true,
            notify_only: false,
        };
        let cycles = run_loop(
            &run,
            &dir,
            settings,
            Duration::from_millis(1),
            Some(2),
            &|_| {},
        );
        assert_eq!(cycles, 2);
        assert_eq!(read_pid(&dir), Some(std::process::id()));
        assert!(sweep::read_status(&dir).is_some());
        cleanup(&dir);
    }
}
