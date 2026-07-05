from herdr_branch_cleanup.procio import RunResult


class FakeRunner:
    """Scripted subprocess replacement: maps argv prefixes to results."""

    def __init__(self):
        self.responses = []
        self.calls = []

    def on(self, *argv_contains, returncode=0, stdout="", stderr=""):
        self.responses.append(
            (argv_contains, RunResult(returncode=returncode, stdout=stdout, stderr=stderr))
        )
        return self

    def __call__(self, argv, cwd=None):
        self.calls.append((list(argv), cwd))
        joined = " ".join(argv)
        for needles, result in self.responses:
            if all(needle in joined for needle in needles):
                return result
        return RunResult(returncode=1, stdout="", stderr=f"no fake response for: {joined}")
