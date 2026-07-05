import json

from tests.conftest import FakeRunner

from herdr_branch_cleanup import gitio


class TestRepoRoot:
    def test_returns_toplevel(self):
        run = FakeRunner().on("rev-parse --show-toplevel", stdout="/repo\n")
        assert gitio.repo_root("/repo/sub", run) == "/repo"

    def test_none_outside_a_repo(self):
        run = FakeRunner().on("rev-parse --show-toplevel", returncode=128)
        assert gitio.repo_root("/tmp", run) is None


class TestCurrentBranch:
    def test_branch_name(self):
        run = FakeRunner().on("branch --show-current", stdout="feature\n")
        assert gitio.current_branch("/repo", run) == "feature"

    def test_empty_when_detached(self):
        run = FakeRunner().on("branch --show-current", stdout="\n")
        assert gitio.current_branch("/repo", run) == ""

    def test_empty_on_error(self):
        run = FakeRunner().on("branch --show-current", returncode=128)
        assert gitio.current_branch("/repo", run) == ""


class TestDefaultBranch:
    def test_from_local_symbolic_ref(self):
        run = FakeRunner().on("symbolic-ref", stdout="origin/main\n")
        assert gitio.default_branch("/repo", run) == "main"

    def test_falls_back_to_remote_symref(self):
        run = (
            FakeRunner()
            .on("symbolic-ref", returncode=128)
            .on(
                "ls-remote --symref",
                stdout="ref: refs/heads/develop\tHEAD\nabc123\tHEAD\n",
            )
        )
        assert gitio.default_branch("/repo", run) == "develop"

    def test_empty_when_remote_unreachable(self):
        run = FakeRunner().on("symbolic-ref", returncode=128).on("ls-remote", returncode=128)
        assert gitio.default_branch("/repo", run) == ""

    def test_empty_when_symref_output_malformed(self):
        run = (
            FakeRunner()
            .on("symbolic-ref", returncode=128)
            .on("ls-remote --symref", stdout="abc123\tHEAD\n")
        )
        assert gitio.default_branch("/repo", run) == ""


class TestGithubOrigin:
    def test_github_url(self):
        run = FakeRunner().on("remote get-url", stdout="git@github.com:me/repo.git\n")
        assert gitio.github_origin("/repo", run) is True

    def test_other_host(self):
        run = FakeRunner().on("remote get-url", stdout="https://gitlab.com/me/repo.git\n")
        assert gitio.github_origin("/repo", run) is False

    def test_no_origin(self):
        run = FakeRunner().on("remote get-url", returncode=2)
        assert gitio.github_origin("/repo", run) is False


class TestLinkedWorktree:
    def test_main_checkout(self):
        run = FakeRunner().on("--git-dir --git-common-dir", stdout=".git\n.git\n")
        assert gitio.linked_worktree("/repo", run) is False

    def test_linked_worktree(self):
        run = FakeRunner().on(
            "--git-dir --git-common-dir",
            stdout="/repo/.git/worktrees/task\n/repo/.git\n",
        )
        assert gitio.linked_worktree("/wt", run) is True

    def test_error_treated_as_not_linked(self):
        run = FakeRunner().on("--git-dir --git-common-dir", returncode=128)
        assert gitio.linked_worktree("/repo", run) is False


class TestDirty:
    def test_clean(self):
        run = FakeRunner().on("status --porcelain", stdout="")
        assert gitio.dirty("/repo", run) is False

    def test_dirty(self):
        run = FakeRunner().on("status --porcelain", stdout=" M file.py\n")
        assert gitio.dirty("/repo", run) is True

    def test_error_treated_as_dirty(self):
        run = FakeRunner().on("status --porcelain", returncode=128)
        assert gitio.dirty("/repo", run) is True


class TestShas:
    def test_tip_sha(self):
        run = FakeRunner().on("rev-parse HEAD", stdout="abc123\n")
        assert gitio.tip_sha("/repo", run) == "abc123"

    def test_tip_sha_error(self):
        run = FakeRunner().on("rev-parse HEAD", returncode=128)
        assert gitio.tip_sha("/repo", run) == ""

    def test_last_known_remote_sha(self):
        run = FakeRunner().on("refs/remotes/origin/feature", stdout="def456\n")
        assert gitio.last_known_remote_sha("/repo", "feature", run) == "def456"

    def test_last_known_remote_sha_missing(self):
        run = FakeRunner().on("refs/remotes/origin/feature", returncode=128)
        assert gitio.last_known_remote_sha("/repo", "feature", run) == ""


class TestRemoteBranchExists:
    def test_exists(self):
        run = FakeRunner().on("ls-remote --heads", stdout="abc\trefs/heads/feature\n")
        assert gitio.remote_branch_exists("/repo", "feature", run) is True

    def test_deleted(self):
        run = FakeRunner().on("ls-remote --heads", stdout="")
        assert gitio.remote_branch_exists("/repo", "feature", run) is False

    def test_unreachable_returns_none(self):
        run = FakeRunner().on("ls-remote --heads", returncode=128)
        assert gitio.remote_branch_exists("/repo", "feature", run) is None


class TestMergedPrHeadSha:
    def test_merged_pr_found(self):
        payload = json.dumps([{"headRefOid": "abc123"}])
        run = FakeRunner().on("gh pr list", stdout=payload)
        assert gitio.merged_pr_head_sha("/repo", "feature", run) == "abc123"

    def test_no_merged_pr(self):
        run = FakeRunner().on("gh pr list", stdout="[]")
        assert gitio.merged_pr_head_sha("/repo", "feature", run) == ""

    def test_gh_failure(self):
        run = FakeRunner().on("gh pr list", returncode=4)
        assert gitio.merged_pr_head_sha("/repo", "feature", run) == ""

    def test_invalid_json(self):
        run = FakeRunner().on("gh pr list", stdout="not json")
        assert gitio.merged_pr_head_sha("/repo", "feature", run) == ""

    def test_unexpected_shape(self):
        run = FakeRunner().on("gh pr list", stdout=json.dumps([{"headRefOid": 7}]))
        assert gitio.merged_pr_head_sha("/repo", "feature", run) == ""

    def test_runs_in_repo_cwd(self):
        run = FakeRunner().on("gh pr list", stdout="[]")
        gitio.merged_pr_head_sha("/repo", "feature", run)
        assert run.calls[0][1] == "/repo"


class TestCheckoutDefault:
    def test_success(self):
        run = FakeRunner().on("checkout main", stdout="").on("pull --ff-only", stdout="")
        assert gitio.checkout_default("/repo", "main", run) is None

    def test_checkout_failure(self):
        run = FakeRunner().on("checkout main", returncode=1, stderr="conflict")
        assert gitio.checkout_default("/repo", "main", run) == "checkout failed: conflict"

    def test_pull_failure_still_reports_checkout(self):
        run = (
            FakeRunner()
            .on("checkout main", stdout="")
            .on("pull --ff-only", returncode=1, stderr="diverged")
        )
        error = gitio.checkout_default("/repo", "main", run)
        assert error == "checked out main, but pull failed: diverged"
