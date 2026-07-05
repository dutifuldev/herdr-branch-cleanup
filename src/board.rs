//! Render the sweep status as a small text board for the plugin pane.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::sweep;

fn glyph(action: &str) -> &'static str {
    match action {
        "checkout" => "✓",
        "hold" => "⏸",
        "skip" => "·",
        _ => "?",
    }
}

pub fn shorten(path: &str, limit: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= limit {
        return path.to_owned();
    }
    let tail: String = chars[chars.len() - (limit - 1)..].iter().collect();
    format!("…{tail}")
}

fn field<'a>(row: &'a serde_json::Value, key: &str) -> &'a str {
    row.get(key).and_then(|value| value.as_str()).unwrap_or("")
}

fn render_report(row: &serde_json::Value) -> String {
    let action = field(row, "action");
    let root = shorten(field(row, "root"), 40);
    let branch = field(row, "branch");
    let branch_label = if branch.is_empty() {
        "(detached)"
    } else {
        branch
    };
    let reason = field(row, "reason");
    format!(
        " {} {}  [{}]  {}",
        glyph(action),
        root,
        branch_label,
        reason
    )
}

fn mode_line(status: &serde_json::Value) -> Option<String> {
    let mut modes = Vec::new();
    if status["dry_run"] == serde_json::Value::Bool(true) {
        modes.push("dry run");
    }
    if status["notify_only"] == serde_json::Value::Bool(true) {
        modes.push("notify only");
    }
    (!modes.is_empty()).then(|| format!(" mode: {}", modes.join(", ")))
}

fn age_line(status: &serde_json::Value) -> Option<String> {
    let updated = status.get("updated_epoch")?.as_f64()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs_f64();
    let age = (now - updated).max(0.0) as u64;
    Some(format!(" last sweep {age}s ago · ctrl+c to close"))
}

pub fn render(status: Option<&serde_json::Value>) -> String {
    let mut lines = vec!["herdr-branch-cleanup".to_owned(), String::new()];
    let Some(status) = status else {
        lines.push(" no sweep has run yet".to_owned());
        return lines.join("\n");
    };
    if let Some(mode) = mode_line(status) {
        lines.push(mode);
    }
    let rows = status
        .get("reports")
        .and_then(|reports| reports.as_array())
        .cloned()
        .unwrap_or_default();
    if rows.is_empty() {
        lines.push(" no git repos in any pane".to_owned());
    }
    lines.extend(rows.iter().map(render_report));
    if let Some(age) = age_line(status) {
        lines.push(String::new());
        lines.push(age);
    }
    lines.join("\n")
}

pub fn board_loop(
    state_dir: &Path,
    refresh: Duration,
    max_frames: Option<u64>,
    sleep: &dyn Fn(Duration),
    emit: &mut dyn FnMut(&str),
) {
    let mut frames = 0;
    loop {
        let status = sweep::read_status(state_dir);
        emit(&format!("\x1b[2J\x1b[H{}", render(status.as_ref())));
        frames += 1;
        if max_frames.is_some_and(|max| frames >= max) {
            return;
        }
        sleep(refresh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status_with(reports: serde_json::Value) -> serde_json::Value {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs_f64();
        json!({"updated_epoch": now, "reports": reports})
    }

    #[test]
    fn no_status_yet() {
        assert!(render(None).contains("no sweep has run yet"));
    }

    #[test]
    fn empty_reports() {
        let status = status_with(json!([]));
        assert!(render(Some(&status)).contains("no git repos in any pane"));
    }

    #[test]
    fn report_rows_with_glyphs() {
        let status = status_with(json!([
            {"root": "/repo", "branch": "feature", "action": "checkout",
             "reason": "branch merged, checked out main"},
            {"root": "/other", "branch": "fix", "action": "hold", "reason": "dirty"},
            {"root": "/third", "branch": "main", "action": "skip", "reason": "on default"},
        ]));
        let text = render(Some(&status));
        assert!(text.contains(" ✓ /repo  [feature]  branch merged, checked out main"));
        assert!(text.contains(" ⏸ /other  [fix]  dirty"));
        assert!(text.contains(" · /third  [main]  on default"));
    }

    #[test]
    fn unknown_action_gets_question_mark() {
        let status = status_with(json!([
            {"root": "/repo", "branch": "b", "action": "wat", "reason": "r"},
        ]));
        assert!(render(Some(&status)).contains(" ? /repo"));
    }

    #[test]
    fn detached_branch_label() {
        let status = status_with(json!([
            {"root": "/repo", "branch": "", "action": "skip", "reason": "detached HEAD"},
        ]));
        assert!(render(Some(&status)).contains("[(detached)]"));
    }

    #[test]
    fn mode_line_shows_dry_run_and_notify() {
        let mut status = status_with(json!([]));
        status["dry_run"] = json!(true);
        status["notify_only"] = json!(true);
        assert!(render(Some(&status)).contains(" mode: dry run, notify only"));
    }

    #[test]
    fn mode_line_absent_in_auto_mode() {
        let status = status_with(json!([]));
        assert!(!render(Some(&status)).contains("mode:"));
    }

    #[test]
    fn age_footer_present() {
        let status = status_with(json!([]));
        assert!(render(Some(&status)).contains("last sweep 0s ago"));
    }

    #[test]
    fn shorten_keeps_short_paths() {
        assert_eq!(shorten("/repo", 40), "/repo");
    }

    #[test]
    fn shorten_keeps_tail_of_long_paths() {
        let path = "/very/long/path/to/some/deeply/nested/repository/checkout";
        let short = shorten(path, 20);
        assert_eq!(short.chars().count(), 20);
        assert!(short.starts_with('…'));
        assert!(short.ends_with("checkout"));
    }

    #[test]
    fn board_loop_renders_frames_from_status_file() {
        let dir =
            std::env::temp_dir().join(format!("herdr-branch-cleanup-board-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        crate::sweep::write_status(
            &dir,
            &[],
            crate::sweep::Settings {
                dry_run: true,
                notify_only: false,
            },
        );
        let mut frames: Vec<String> = Vec::new();
        board_loop(
            &dir,
            Duration::from_millis(1),
            Some(2),
            &|_| {},
            &mut |text| {
                frames.push(text.to_owned());
            },
        );
        assert_eq!(frames.len(), 2);
        assert!(frames[0].contains("dry run"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
