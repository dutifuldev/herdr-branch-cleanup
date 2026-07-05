#!/usr/bin/env python3
"""Zero-install launcher: herdr runs this with the plugin dir as cwd."""

import sys
from pathlib import Path

PLUGIN_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PLUGIN_ROOT / "src"))

from herdr_branch_cleanup.cli import main  # noqa: E402

if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:], str(Path(__file__).resolve())))
