import sys

from herdr_branch_cleanup import procio


class TestRun:
    def test_captures_stdout(self):
        result = procio.run([sys.executable, "-c", "print('hello')"])
        assert result.ok is True
        assert result.text == "hello"

    def test_captures_failure(self):
        result = procio.run([sys.executable, "-c", "import sys; sys.exit(3)"])
        assert result.ok is False
        assert result.returncode == 3

    def test_missing_binary_reported_not_raised(self):
        result = procio.run(["definitely-not-a-real-binary-xyz"])
        assert result.ok is False
        assert result.returncode == 127
        assert result.stderr != ""

    def test_cwd_is_respected(self, tmp_path):
        result = procio.run(
            [sys.executable, "-c", "import os; print(os.getcwd())"], cwd=str(tmp_path)
        )
        assert result.text == str(tmp_path)
