import json
import os

from herdr_branch_cleanup import daemon, sweep
from tests.conftest import FakeRunner

SETTINGS = sweep.Settings(dry_run=True, notify_only=False)


class FakeSpawner:
    def __init__(self, pid=4321):
        self.pid = pid
        self.calls = []

    def __call__(self, argv, log_path):
        self.calls.append((argv, log_path))
        return self.pid


class TestStateDir:
    def test_uses_plugin_state_dir(self, monkeypatch):
        monkeypatch.setenv("HERDR_PLUGIN_STATE_DIR", "/tmp/state")
        assert str(daemon.state_dir()) == "/tmp/state"

    def test_falls_back_to_local(self, monkeypatch):
        monkeypatch.delenv("HERDR_PLUGIN_STATE_DIR", raising=False)
        assert str(daemon.state_dir()) == ".state"


class TestPidHandling:
    def test_read_pid_missing(self, tmp_path):
        assert daemon.read_pid(tmp_path) is None

    def test_read_pid_garbage(self, tmp_path):
        (tmp_path / daemon.PID_FILE).write_text("nope", encoding="utf-8")
        assert daemon.read_pid(tmp_path) is None

    def test_running_pid_for_live_process(self, tmp_path):
        (tmp_path / daemon.PID_FILE).write_text(str(os.getpid()), encoding="utf-8")
        assert daemon.running_pid(tmp_path) == os.getpid()

    def test_running_pid_for_dead_process(self, tmp_path):
        (tmp_path / daemon.PID_FILE).write_text("999999", encoding="utf-8")
        assert daemon.running_pid(tmp_path) is None


class TestStartStop:
    def test_start_spawns_and_writes_pidfile(self, tmp_path):
        spawner = FakeSpawner()
        message = daemon.start(tmp_path, "/plugin/scripts/run.py", spawner)
        assert message == "started (pid 4321)"
        assert daemon.read_pid(tmp_path) == 4321
        argv, log_path = spawner.calls[0]
        assert argv[-2:] == ["/plugin/scripts/run.py", "run"]
        assert log_path == tmp_path / daemon.LOG_FILE

    def test_start_is_idempotent_when_running(self, tmp_path):
        (tmp_path / daemon.PID_FILE).write_text(str(os.getpid()), encoding="utf-8")
        spawner = FakeSpawner()
        message = daemon.start(tmp_path, "run.py", spawner)
        assert message.startswith("already running")
        assert spawner.calls == []

    def test_stop_when_not_running(self, tmp_path):
        assert daemon.stop(tmp_path) == "not running"

    def test_toggle_starts_when_stopped(self, tmp_path):
        message = daemon.toggle(tmp_path, "run.py", FakeSpawner())
        assert message.startswith("started")

    def test_status_reports_both_states(self, tmp_path):
        assert daemon.status(tmp_path) == "not running"
        (tmp_path / daemon.PID_FILE).write_text(str(os.getpid()), encoding="utf-8")
        assert daemon.status(tmp_path) == f"running (pid {os.getpid()})"


class TestNudge:
    def test_nudge_touches_marker(self, tmp_path):
        assert daemon.nudge_stamp(tmp_path) == 0.0
        daemon.nudge(tmp_path)
        assert daemon.nudge_stamp(tmp_path) > 0.0

    def test_wait_wakes_early_on_nudge(self, tmp_path):
        slept = []

        def fake_sleep(seconds):
            slept.append(seconds)
            daemon.nudge(tmp_path)

        stamp = daemon.wait_for_next_cycle(
            tmp_path, interval=60.0, seen_stamp=0.0, sleep=fake_sleep
        )
        assert len(slept) == 1
        assert stamp > 0.0

    def test_wait_runs_full_interval_without_nudge(self, tmp_path):
        slept = []
        stamp = daemon.wait_for_next_cycle(
            tmp_path, interval=2.0, seen_stamp=0.0, sleep=slept.append
        )
        assert stamp == 0.0
        assert sum(slept) >= 2.0


class TestRunLoop:
    def test_runs_requested_cycles(self, tmp_path, monkeypatch):
        run = FakeRunner().on("pane list", stdout=json.dumps({"result": {"panes": []}}))
        original_sweep_once = sweep.sweep_once
        monkeypatch.setattr(sweep, "sweep_once", lambda d, s: original_sweep_once(d, s, run))
        cycles = daemon.run_loop(
            tmp_path, SETTINGS, interval=0.1, max_cycles=2, sleep=lambda _: None
        )
        assert cycles == 2
        assert daemon.read_pid(tmp_path) == os.getpid()
        assert sweep.read_status(tmp_path) is not None
