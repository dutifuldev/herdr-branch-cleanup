import json

from herdr_branch_cleanup import sweep
from herdr_branch_cleanup.herdrio import Pane
from tests.conftest import FakeRunner

AUTO = sweep.Settings(dry_run=False, notify_only=False)
DRY = sweep.Settings(dry_run=True, notify_only=False)
NOTIFY = sweep.Settings(dry_run=False, notify_only=True)


def merged_branch_runner(root="/repo"):
    """A repo on a clean feature branch whose PR was merged on GitHub."""
    return (
        FakeRunner()
        .on("rev-parse --show-toplevel", stdout=f"{root}\n")
        .on("branch --show-current", stdout="feature\n")
        .on("symbolic-ref", stdout="origin/main\n")
        .on("remote get-url", stdout="git@github.com:me/repo.git\n")
        .on("--git-dir --git-common-dir", stdout=".git\n.git\n")
        .on("status --porcelain", stdout="")
        .on("rev-parse HEAD", stdout="abc123\n")
        .on("ls-remote --heads", stdout="")
        .on("gh pr list", stdout=json.dumps([{"headRefOid": "abc123"}]))
        .on("refs/remotes/origin/feature", stdout="abc123\n")
        .on("checkout main", stdout="")
        .on("pull --ff-only", stdout="")
    )


def pane(cwd="/repo", status="idle", pane_id="w1-1"):
    return Pane(pane_id=pane_id, cwd=cwd, agent_status=status)


class TestReposFromPanes:
    def test_groups_panes_by_repo_root(self):
        run = (
            FakeRunner()
            .on("-C /repo/sub rev-parse --show-toplevel", stdout="/repo\n")
            .on("-C /repo rev-parse --show-toplevel", stdout="/repo\n")
        )
        panes = [pane(cwd="/repo"), pane(cwd="/repo/sub", pane_id="w1-2")]
        repos = sweep.repos_from_panes(panes, run)
        assert list(repos) == ["/repo"]
        assert len(repos["/repo"]) == 2

    def test_non_repo_panes_dropped(self):
        run = FakeRunner().on("rev-parse --show-toplevel", returncode=128)
        assert sweep.repos_from_panes([pane(cwd="/tmp")], run) == {}

    def test_repo_root_resolved_once_per_cwd(self):
        run = FakeRunner().on("rev-parse --show-toplevel", stdout="/repo\n")
        sweep.repos_from_panes([pane(), pane(pane_id="w1-2")], run)
        assert len(run.calls) == 1


class TestSweepRepo:
    def test_checks_out_merged_branch(self):
        run = merged_branch_runner()
        report = sweep.sweep_repo("/repo", [pane()], AUTO, run)
        assert report.action == "checkout"
        assert report.checked_out is True
        assert "checked out main" in report.reason
        assert any("checkout" in " ".join(argv) for argv, _ in run.calls)

    def test_dry_run_reports_without_acting(self):
        run = merged_branch_runner()
        report = sweep.sweep_repo("/repo", [pane()], DRY, run)
        assert report.checked_out is False
        assert "dry run" in report.reason
        assert not any("checkout main" in " ".join(argv) for argv, _ in run.calls)

    def test_notify_only_reports_without_acting(self):
        run = merged_branch_runner()
        report = sweep.sweep_repo("/repo", [pane()], NOTIFY, run)
        assert report.checked_out is False
        assert "notify-only" in report.reason

    def test_local_skip_short_circuits_remote_checks(self):
        run = (
            FakeRunner()
            .on("branch --show-current", stdout="main\n")
            .on("symbolic-ref", stdout="origin/main\n")
            .on("remote get-url", stdout="git@github.com:me/repo.git\n")
            .on("--git-dir --git-common-dir", stdout=".git\n.git\n")
            .on("status --porcelain", stdout="")
            .on("rev-parse HEAD", stdout="abc123\n")
        )
        report = sweep.sweep_repo("/repo", [pane()], AUTO, run)
        assert report.action == "skip"
        assert report.reason == "already on default branch"
        assert not any("ls-remote --heads" in " ".join(argv) for argv, _ in run.calls)

    def test_unreachable_remote_skips(self):
        run = merged_branch_runner()
        run.responses = [r for r in run.responses if "ls-remote --heads" not in r[0]]
        run.on("ls-remote --heads", returncode=128)
        report = sweep.sweep_repo("/repo", [pane()], AUTO, run)
        assert report.action == "skip"
        assert report.reason == "remote unreachable"

    def test_busy_agent_holds(self):
        run = merged_branch_runner()
        report = sweep.sweep_repo("/repo", [pane(status="working")], AUTO, run)
        assert report.action == "hold"
        assert report.checked_out is False

    def test_checkout_failure_reported(self):
        run = merged_branch_runner()
        run.responses = [r for r in run.responses if "checkout main" not in r[0]]
        run.on("checkout main", returncode=1, stderr="conflict")
        report = sweep.sweep_repo("/repo", [pane()], AUTO, run)
        assert report.checked_out is False
        assert "checkout failed" in report.reason


class TestSweepOnce:
    def test_writes_status_file(self, tmp_path):
        run = merged_branch_runner()
        payload = {"id": "x", "result": {"panes": [{"pane_id": "w1-1", "cwd": "/repo"}]}}
        run.on("pane list", stdout=json.dumps(payload))
        reports = sweep.sweep_once(tmp_path, DRY, run)
        assert len(reports) == 1
        status = sweep.read_status(tmp_path)
        assert status is not None
        assert status["dry_run"] is True
        assert status["reports"][0]["branch"] == "feature"

    def test_no_panes_writes_empty_status(self, tmp_path):
        run = FakeRunner().on("pane list", stdout=json.dumps({"result": {"panes": []}}))
        assert sweep.sweep_once(tmp_path, AUTO, run) == []
        assert sweep.read_status(tmp_path)["reports"] == []


class TestReadStatus:
    def test_missing_file(self, tmp_path):
        assert sweep.read_status(tmp_path) is None

    def test_corrupt_file(self, tmp_path):
        (tmp_path / sweep.STATUS_FILE).write_text("not json", encoding="utf-8")
        assert sweep.read_status(tmp_path) is None

    def test_non_dict_payload(self, tmp_path):
        (tmp_path / sweep.STATUS_FILE).write_text("[]", encoding="utf-8")
        assert sweep.read_status(tmp_path) is None
