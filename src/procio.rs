//! Subprocess boundary: the one place that actually spawns processes.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl RunResult {
    pub fn failure(message: String) -> Self {
        RunResult {
            code: 127,
            stdout: String::new(),
            stderr: message,
        }
    }

    pub fn ok(&self) -> bool {
        self.code == 0
    }

    pub fn text(&self) -> String {
        self.stdout.trim().to_owned()
    }
}

pub trait Runner {
    fn run(&self, argv: &[String], cwd: Option<&Path>) -> RunResult;
}

pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, argv: &[String], cwd: Option<&Path>) -> RunResult {
        let Some((program, args)) = argv.split_first() else {
            return RunResult::failure("empty argv".to_owned());
        };
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(directory) = cwd {
            command.current_dir(directory);
        }
        match command.spawn() {
            Ok(child) => wait_with_timeout(child),
            Err(error) => RunResult::failure(error.to_string()),
        }
    }
}

fn wait_with_timeout(mut child: Child) -> RunResult {
    let stdout = child.stdout.take().map(reader_thread);
    let stderr = child.stderr.take().map(reader_thread);
    let code = poll_until_exit(&mut child);
    RunResult {
        code,
        stdout: join_reader(stdout),
        stderr: join_reader(stderr),
    }
}

fn poll_until_exit(child: &mut Child) -> i32 {
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code().unwrap_or(-1),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return 124;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return 127,
        }
    }
}

fn reader_thread<R: Read + Send + 'static>(mut source: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buffer = String::new();
        let _ = source.read_to_string(&mut buffer);
        buffer
    })
}

fn join_reader(handle: Option<std::thread::JoinHandle<String>>) -> String {
    handle
        .and_then(|reader| reader.join().ok())
        .unwrap_or_default()
}

pub fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_stdout_of_a_real_process() {
        let result = SystemRunner.run(&argv(&["echo", "hello"]), None);
        assert!(result.ok());
        assert_eq!(result.text(), "hello");
    }

    #[test]
    fn captures_failure_exit_code() {
        let result = SystemRunner.run(&argv(&["false"]), None);
        assert!(!result.ok());
    }

    #[test]
    fn missing_binary_reported_not_panicked() {
        let result = SystemRunner.run(&argv(&["definitely-not-a-real-binary-xyz"]), None);
        assert_eq!(result.code, 127);
        assert!(!result.stderr.is_empty());
    }

    #[test]
    fn empty_argv_reported() {
        let result = SystemRunner.run(&[], None);
        assert_eq!(result.code, 127);
    }

    #[test]
    fn cwd_is_respected() {
        let dir = std::env::temp_dir();
        let result = SystemRunner.run(&argv(&["pwd"]), Some(&dir));
        let canonical = dir.canonicalize().expect("canonical temp dir");
        assert_eq!(
            std::path::Path::new(&result.text()).canonicalize().ok(),
            Some(canonical)
        );
    }
}
