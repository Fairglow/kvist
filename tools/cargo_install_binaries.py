#!/usr/bin/env python3
"""Extract and install built binaries from cargo's NDJSON output.

Usage: install_binaries.py <log-file> [install-dir...]
       Defaults to ~/.kvist/bin if install-dir is not specified.
       Multiple install directories are supported.
       The project's target directory is automatically added as an install
       location unless it matches the source or an explicit install dir.
       Copies are silently skipped if source and destination are identical.
"""

import json
import os
import shutil
import sys
from pathlib import Path


def detect_target_dir(log_file, lines):
    """Detect the project's target directory from NDJSON output."""
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") == "compiler-artifact":
            src = msg.get("executable")
            if src:
                # Walk up to find the target directory
                path = Path(src)
                for part in path.parts:
                    if part == "target":
                        return path.parent
                        break
    return None


def main():
    if len(sys.argv) < 2:
        print("Usage: install_binaries.py <log-file> [install-dir...]", file=sys.stderr)
        sys.exit(1)

    log_file = sys.argv[1]
    user_dirs = [Path(d) for d in sys.argv[2:]] if len(sys.argv) > 2 else [Path.home() / ".kvist" / "bin"]

    with open(log_file) as f:
        lines = f.readlines()

    # Detect target directory and add it as an install location
    target_dir = detect_target_dir(log_file, lines)
    install_dirs = list(user_dirs)
    if target_dir is not None:
        normalized_target = target_dir.resolve()
        should_add = True
        for d in install_dirs:
            if d.resolve() == normalized_target:
                should_add = False
                break
        if should_add:
            install_dirs.append(target_dir)

    installed = 0
    skipped = 0
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
            name = os.path.basename(src)
            src_path = Path(src).resolve()

            for install_dir in install_dirs:
                dst = install_dir / name
                dst_path = dst.resolve()
                if src_path == dst_path:
                    skipped += 1
                    continue
                try:
                    install_dir.mkdir(parents=True, exist_ok=True)
                    shutil.copy2(src, dst)
                    installed += 1
                    print(f"Installed {name} -> {dst}")
                except OSError as e:
                    print(f"Warning: failed to install {src} to {dst}: {e}", file=sys.stderr)

    if installed == 0 and skipped == 0:
        print(
            f"Warning: no binaries found to install. "
            f"Saw {artifacts_seen} compiler artifacts but none had executable paths. "
            f"Check that the build produced binaries (e.g., [[bin]] targets in Cargo.toml).",
            file=sys.stderr,
        )
    if skipped > 0:
        print(f"Skipped {skipped} binary copies (source and destination identical)", file=sys.stderr)


if __name__ == "__main__":
    main()