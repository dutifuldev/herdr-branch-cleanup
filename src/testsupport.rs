//! Scripted subprocess replacement for tests: maps argv substrings to results.

use std::cell::RefCell;
use std::path::Path;

use crate::procio::{RunResult, Runner};

pub struct FakeRunner {
    responses: Vec<(Vec<String>, RunResult)>,
    calls: RefCell<Vec<(Vec<String>, Option<String>)>>,
}

impl FakeRunner {
    pub fn new() -> Self {
        FakeRunner {
            responses: Vec::new(),
            calls: RefCell::new(Vec::new()),
        }
    }

    /// Register a response for any command whose joined argv contains every
    /// whitespace-separated needle in `pattern`.
    #[must_use]
    pub fn on(mut self, pattern: &str, code: i32, stdout: &str, stderr: &str) -> Self {
        let needles = pattern.split_whitespace().map(str::to_owned).collect();
        self.responses.push((
            needles,
            RunResult {
                code,
                stdout: stdout.to_owned(),
                stderr: stderr.to_owned(),
            },
        ));
        self
    }

    /// Remove the response registered with exactly this pattern.
    pub fn drop_response(&mut self, pattern: &str) {
        let target: Vec<String> = pattern.split_whitespace().map(str::to_owned).collect();
        self.responses.retain(|(needles, _)| *needles != target);
    }

    pub fn calls(&self) -> Vec<(Vec<String>, Option<String>)> {
        self.calls.borrow().clone()
    }
}

impl Default for FakeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner for FakeRunner {
    fn run(&self, argv: &[String], cwd: Option<&Path>) -> RunResult {
        self.calls.borrow_mut().push((
            argv.to_vec(),
            cwd.map(|path| path.to_string_lossy().into_owned()),
        ));
        let joined = argv.join(" ");
        for (needles, result) in &self.responses {
            if needles
                .iter()
                .all(|needle| joined.contains(needle.as_str()))
            {
                return result.clone();
            }
        }
        RunResult {
            code: 1,
            stdout: String::new(),
            stderr: format!("no fake response for: {joined}"),
        }
    }
}
