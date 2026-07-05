//! Herdr CLI adapter: read the live session through HERDR_BIN_PATH.

use crate::procio::Runner;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub pane_id: String,
    pub cwd: String,
    pub agent_status: String,
}

pub fn herdr_bin() -> String {
    std::env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_owned())
}

pub fn list_panes(run: &dyn Runner) -> Vec<Pane> {
    let result = run.run(&[herdr_bin(), "pane".to_owned(), "list".to_owned()], None);
    if !result.ok() {
        return Vec::new();
    }
    parse_panes(&result.stdout)
}

fn parse_panes(payload: &str) -> Vec<Pane> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return Vec::new();
    };
    let rows = value
        .get("result")
        .and_then(|result| result.get("panes"))
        .and_then(|panes| panes.as_array());
    rows.map(|rows| rows.iter().filter_map(parse_pane).collect())
        .unwrap_or_default()
}

fn parse_pane(row: &serde_json::Value) -> Option<Pane> {
    let cwd = non_empty_str(row, "foreground_cwd").or_else(|| non_empty_str(row, "cwd"))?;
    Some(Pane {
        pane_id: non_empty_str(row, "pane_id").unwrap_or_default(),
        cwd,
        agent_status: non_empty_str(row, "agent_status").unwrap_or_else(|| "unknown".to_owned()),
    })
}

fn non_empty_str(row: &serde_json::Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(|value| value.as_str())
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsupport::FakeRunner;

    fn payload(panes: &str) -> String {
        format!(r#"{{"id":"cli:pane:list","result":{{"panes":{panes}}}}}"#)
    }

    #[test]
    fn parses_panes_preferring_foreground_cwd() {
        let body = payload(
            r#"[
              {"pane_id":"w1-1","cwd":"/home/me","foreground_cwd":"/home/me/repo","agent_status":"idle"},
              {"pane_id":"w1-2","cwd":"/home/me","agent_status":"working"}
            ]"#,
        );
        let run = FakeRunner::new().on("pane list", 0, &body, "");
        let panes = list_panes(&run);
        assert_eq!(
            panes,
            vec![
                Pane {
                    pane_id: "w1-1".to_owned(),
                    cwd: "/home/me/repo".to_owned(),
                    agent_status: "idle".to_owned(),
                },
                Pane {
                    pane_id: "w1-2".to_owned(),
                    cwd: "/home/me".to_owned(),
                    agent_status: "working".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn missing_agent_status_defaults_to_unknown() {
        let body = payload(r#"[{"pane_id":"w1-1","cwd":"/a"}]"#);
        let run = FakeRunner::new().on("pane list", 0, &body, "");
        assert_eq!(list_panes(&run)[0].agent_status, "unknown");
    }

    #[test]
    fn panes_without_cwd_are_dropped() {
        let body = payload(r#"[{"pane_id":"w1-1","agent_status":"idle"}]"#);
        let run = FakeRunner::new().on("pane list", 0, &body, "");
        assert!(list_panes(&run).is_empty());
    }

    #[test]
    fn non_object_rows_are_dropped() {
        let body = payload(r#"["garbage"]"#);
        let run = FakeRunner::new().on("pane list", 0, &body, "");
        assert!(list_panes(&run).is_empty());
    }

    #[test]
    fn command_failure_returns_empty() {
        let run = FakeRunner::new().on("pane list", 1, "", "");
        assert!(list_panes(&run).is_empty());
    }

    #[test]
    fn invalid_json_returns_empty() {
        let run = FakeRunner::new().on("pane list", 0, "not json", "");
        assert!(list_panes(&run).is_empty());
    }

    #[test]
    fn non_object_payload_returns_empty() {
        let run = FakeRunner::new().on("pane list", 0, "[]", "");
        assert!(list_panes(&run).is_empty());
    }
}
