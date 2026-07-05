import json

from herdr_branch_cleanup import herdrio
from tests.conftest import FakeRunner


def pane_list_payload(panes):
    return json.dumps({"id": "cli:pane:list", "result": {"panes": panes}})


class TestListPanes:
    def test_parses_panes(self):
        payload = pane_list_payload(
            [
                {
                    "pane_id": "w1-1",
                    "cwd": "/home/me",
                    "foreground_cwd": "/home/me/repo",
                    "agent_status": "idle",
                },
                {"pane_id": "w1-2", "cwd": "/home/me", "agent_status": "working"},
            ]
        )
        run = FakeRunner().on("pane list", stdout=payload)
        panes = herdrio.list_panes(run)
        assert panes == [
            herdrio.Pane(pane_id="w1-1", cwd="/home/me/repo", agent_status="idle"),
            herdrio.Pane(pane_id="w1-2", cwd="/home/me", agent_status="working"),
        ]

    def test_foreground_cwd_preferred_over_cwd(self):
        payload = pane_list_payload(
            [{"pane_id": "w1-1", "cwd": "/a", "foreground_cwd": "/b", "agent_status": "idle"}]
        )
        run = FakeRunner().on("pane list", stdout=payload)
        assert herdrio.list_panes(run)[0].cwd == "/b"

    def test_missing_agent_status_defaults_to_unknown(self):
        payload = pane_list_payload([{"pane_id": "w1-1", "cwd": "/a"}])
        run = FakeRunner().on("pane list", stdout=payload)
        assert herdrio.list_panes(run)[0].agent_status == "unknown"

    def test_panes_without_cwd_are_dropped(self):
        payload = pane_list_payload([{"pane_id": "w1-1", "agent_status": "idle"}])
        run = FakeRunner().on("pane list", stdout=payload)
        assert herdrio.list_panes(run) == []

    def test_non_dict_rows_are_dropped(self):
        payload = pane_list_payload(["garbage"])
        run = FakeRunner().on("pane list", stdout=payload)
        assert herdrio.list_panes(run) == []

    def test_command_failure_returns_empty(self):
        run = FakeRunner().on("pane list", returncode=1)
        assert herdrio.list_panes(run) == []

    def test_invalid_json_returns_empty(self):
        run = FakeRunner().on("pane list", stdout="not json")
        assert herdrio.list_panes(run) == []

    def test_non_dict_payload_returns_empty(self):
        run = FakeRunner().on("pane list", stdout="[]")
        assert herdrio.list_panes(run) == []


class TestHerdrBin:
    def test_uses_env_override(self, monkeypatch):
        monkeypatch.setenv("HERDR_BIN_PATH", "/opt/herdr")
        assert herdrio.herdr_bin() == "/opt/herdr"

    def test_falls_back_to_path_lookup(self, monkeypatch):
        monkeypatch.delenv("HERDR_BIN_PATH", raising=False)
        assert herdrio.herdr_bin() == "herdr"
