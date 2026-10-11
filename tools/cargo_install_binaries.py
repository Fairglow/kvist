#!/usr/bin/env python3
"""Extract and install built binaries from cargo's NDJSON output.

Usage: install_binaries.py <log-file> [install-dir]
       Defaults to ~/.kvist/bin if install-dir is not specified.
"""

import json
import os
import shutil
import sys
from pathlib import Path


def main():
    if len(sys.argv) < 2:
        print("Usage: install_binaries.py <log-file> [install-dir]", file=sys.stderr)
        sys.exit(1)

    log_file = sys.argv[1]
    install_dir = (
        Path(sys.argv[2]) if len(sys.argv) > 2 else Path.home() / ".kvist" / "bin"
    )

    with open(log_file) as f:
        lines = f.readlines()

    installed = 0
    artifacts_seen = 0

    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") == "compiler-artifact":
            artifacts_seen += 1
            src = msg.get("executable")
            if src is None:
                continue  # Not a binary target
            try:
                name = os.path.basename(src)
                dst = install_dir / name
                install_dir.mkdir(parents=True, exist_ok=True)
                shutil.copy2(src, dst)
                installed += 1
                print(f"Installed {name} -> {dst}")
            except OSError as e:
                print(f"Warning: failed to install {src}: {e}", file=sys.stderr)

    if installed == 0:
        print(
            f"Warning: no binaries found to install. "
            f"Saw {artifacts_seen} compiler artifacts but none had executable paths. "
            f"Check that the build produced binaries (e.g., [[bin]] targets in Cargo.toml).",
            file=sys.stderr,
        )


if __name__ == "__main__":
    main()
