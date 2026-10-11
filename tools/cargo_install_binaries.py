#!/usr/bin/env python3
"""Extract and install built binaries from cargo's NDJSON output.

Usage: install_binaries.py <log-file> [install-dir]
       Defaults to ~/.kvist/bin if install-dir is not specified.
"""
import sys
import json
import os
import shutil
from pathlib import Path


def main():
    if len(sys.argv) < 2:
        print("Usage: install_binaries.py <log-file> [install-dir]", file=sys.stderr)
        sys.exit(1)

    log_file = sys.argv[1]
    install_dir = Path(sys.argv[2]) if len(sys.argv) > 2 else Path.home() / ".kvist" / "bin"

    with open(log_file) as f:
        lines = f.readlines()

    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") == "compiler-artifact" and "executable" in msg:
            src = msg["executable"]
            name = os.path.basename(src)
            dst = install_dir / name
            install_dir.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
            print(f"Installed {name} -> {dst}")


if __name__ == "__main__":
    main()
