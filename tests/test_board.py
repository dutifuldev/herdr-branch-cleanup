import time

from herdr_branch_cleanup import board, sweep


def status_with(reports, **extra):
    payload = {"updated_epoch": time.time(), "reports": reports}
    payload.update(extra)
    return payload


class TestRender:
    def test_no_status_yet(self):
        text = board.render(None)
        assert "no sweep has run yet" in text

    def test_empty_reports(self):
        text = board.render(status_with([]))
        assert "no git repos in any pane" in text

    def test_report_rows_with_glyphs(self):
        reports = [
            {
                "root": "/repo",
                "branch": "feature",
                "action": "checkout",
                "reason": "branch merged, checked out main",
            },
            {"root": "/other", "branch": "fix", "action": "hold", "reason": "dirty"},
            {"root": "/third", "branch": "main", "action": "skip", "reason": "on default"},
        ]
        text = board.render(status_with(reports))
        assert " ✓ /repo  [feature]  branch merged, checked out main" in text
        assert " ⏸ /other  [fix]  dirty" in text
        assert " · /third  [main]  on default" in text

    def test_detached_branch_label(self):
        reports = [{"root": "/repo", "branch": "", "action": "skip", "reason": "detached HEAD"}]
        assert "[(detached)]" in board.render(status_with(reports))

    def test_mode_line_shows_dry_run_and_notify(self):
        text = board.render(status_with([], dry_run=True, notify_only=True))
        assert " mode: dry run, notify only" in text

    def test_mode_line_absent_in_auto_mode(self):
        assert "mode:" not in board.render(status_with([]))

    def test_age_footer(self):
        text = board.render(status_with([]))
        assert "last sweep 0s ago" in text


class TestShorten:
    def test_short_path_unchanged(self):
        assert board.shorten("/repo") == "/repo"

    def test_long_path_keeps_tail(self):
        path = "/very/long/path/to/some/deeply/nested/repository/checkout"
        shortened = board.shorten(path, limit=20)
        assert len(shortened) == 20
        assert shortened.startswith("…")
        assert shortened.endswith("checkout")


class TestBoardLoop:
    def test_renders_frames_from_status_file(self, tmp_path):
        sweep.write_status(tmp_path, [], sweep.Settings(dry_run=True, notify_only=False))
        frames = []
        board.board_loop(tmp_path, max_frames=2, sleep=lambda _: None, emit=frames.append)
        assert len(frames) == 2
        assert "dry run" in frames[0]
