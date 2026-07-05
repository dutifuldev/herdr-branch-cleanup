import json

from herdr_branch_cleanup import board, cli, daemon, sweep


class TestConfigEnv:
    def test_env_wins_over_config_file(self, tmp_path):
        (tmp_path / "config.env").write_text(
            "BRANCH_CLEANUP_INTERVAL=30\nBRANCH_CLEANUP_DRY_RUN=1\n", encoding="utf-8"
        )
        env = cli.config_env(
            {"HERDR_PLUGIN_CONFIG_DIR": str(tmp_path), "BRANCH_CLEANUP_INTERVAL": "90"}
        )
        assert env["BRANCH_CLEANUP_INTERVAL"] == "90"
        assert env["BRANCH_CLEANUP_DRY_RUN"] == "1"

    def test_no_config_dir(self):
        assert cli.config_env({"A": "1"}) == {"A": "1"}

    def test_missing_config_file(self, tmp_path):
        env = cli.config_env({"HERDR_PLUGIN_CONFIG_DIR": str(tmp_path)})
        assert "BRANCH_CLEANUP_INTERVAL" not in env


class TestParseEnvFile:
    def test_parses_and_skips_comments(self, tmp_path):
        path = tmp_path / "config.env"
        path.write_text("# comment\n\nKEY = value\nBROKEN LINE\n", encoding="utf-8")
        assert cli.parse_env_file(path) == {"KEY": "value"}


class TestSettings:
    def test_defaults(self):
        settings = cli.settings_from({})
        assert settings == sweep.Settings(dry_run=False, notify_only=False)

    def test_truthy_values(self):
        settings = cli.settings_from(
            {"BRANCH_CLEANUP_DRY_RUN": "TRUE", "BRANCH_CLEANUP_NOTIFY_ONLY": "yes"}
        )
        assert settings == sweep.Settings(dry_run=True, notify_only=True)

    def test_falsy_values(self):
        settings = cli.settings_from({"BRANCH_CLEANUP_DRY_RUN": "0"})
        assert settings.dry_run is False


class TestInterval:
    def test_default(self):
        assert cli.interval_from({}) == 60.0

    def test_configured(self):
        assert cli.interval_from({"BRANCH_CLEANUP_INTERVAL": "120"}) == 120.0

    def test_floor_of_five_seconds(self):
        assert cli.interval_from({"BRANCH_CLEANUP_INTERVAL": "1"}) == 5.0

    def test_garbage_falls_back(self):
        assert cli.interval_from({"BRANCH_CLEANUP_INTERVAL": "soon"}) == 60.0


class TestMain:
    def test_status_command(self, tmp_path, monkeypatch, capsys):
        monkeypatch.setenv("HERDR_PLUGIN_STATE_DIR", str(tmp_path))
        assert cli.main(["status"], "run.py") == 0
        assert "not running" in capsys.readouterr().out

    def test_start_and_stop_lifecycle(self, tmp_path, monkeypatch, capsys):
        monkeypatch.setenv("HERDR_PLUGIN_STATE_DIR", str(tmp_path))
        monkeypatch.setattr(daemon, "spawn_detached", lambda argv, log_path: 424242)
        assert cli.main(["start"], "run.py") == 0
        assert "started (pid 424242)" in capsys.readouterr().out
        assert cli.main(["stop"], "run.py") == 0
        assert "not running" in capsys.readouterr().out

    def test_toggle_command(self, tmp_path, monkeypatch, capsys):
        monkeypatch.setenv("HERDR_PLUGIN_STATE_DIR", str(tmp_path))
        monkeypatch.setattr(daemon, "spawn_detached", lambda argv, log_path: 424242)
        assert cli.main(["toggle"], "run.py") == 0
        assert "started" in capsys.readouterr().out

    def test_nudge_command(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HERDR_PLUGIN_STATE_DIR", str(tmp_path))
        assert cli.main(["nudge"], "run.py") == 0
        assert daemon.nudge_stamp(tmp_path) > 0.0

    def test_once_command_prints_reports(self, tmp_path, monkeypatch, capsys):
        monkeypatch.setenv("HERDR_PLUGIN_STATE_DIR", str(tmp_path))
        monkeypatch.setenv("BRANCH_CLEANUP_DRY_RUN", "1")
        payload = json.dumps({"result": {"panes": []}})

        def fake_sweep(state_dir, settings, run=None):
            sweep.write_status(state_dir, [], settings)
            return []

        monkeypatch.setattr(sweep, "sweep_once", fake_sweep)
        assert cli.main(["once"], "run.py") == 0
        assert payload is not None

    def test_run_command_enters_loop(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HERDR_PLUGIN_STATE_DIR", str(tmp_path))
        calls = {}

        def fake_loop(directory, settings, interval, max_cycles=None, sleep=None):
            calls["interval"] = interval
            return 0

        monkeypatch.setattr(daemon, "run_loop", fake_loop)
        assert cli.main(["run"], "run.py") == 0
        assert calls["interval"] == 60.0

    def test_board_command(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HERDR_PLUGIN_STATE_DIR", str(tmp_path))
        called = {}
        monkeypatch.setattr(
            board, "board_loop", lambda directory: called.setdefault("d", directory)
        )
        assert cli.main(["board"], "run.py") == 0
        assert called["d"] == tmp_path

    def test_keyboard_interrupt_exits_130(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HERDR_PLUGIN_STATE_DIR", str(tmp_path))

        def raise_interrupt(directory):
            raise KeyboardInterrupt

        monkeypatch.setattr(board, "board_loop", raise_interrupt)
        assert cli.main(["board"], "run.py") == 130
