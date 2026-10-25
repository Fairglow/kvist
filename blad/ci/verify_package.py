#!/usr/bin/env python3
"""Verify the actual standalone crate, without a Kvist checkout at build time.

Requires Python 3.12+, Cargo and the documented Linux PTY test prerequisites.
Run from any directory. --allow-dirty is for local candidate preparation only.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib


def run(arguments, **kwargs):
    print("+", " ".join(str(argument) for argument in arguments), flush=True)
    return subprocess.run(arguments, check=True, **kwargs)


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--allow-dirty", action="store_true")
    args = parser.parse_args()
    component = Path(__file__).resolve().parents[1]
    metadata = tomllib.loads((component / "Cargo.toml").read_text())
    name = metadata["package"]["name"]
    version = metadata["package"]["version"]
    require(name == "kvist-blad", "unexpected package identity")
    cargo = ["cargo", "--offline"] if args.offline else ["cargo"]
    info = run(
        cargo + ["metadata", "--locked", "--no-deps", "--format-version", "1",
                 "--manifest-path", str(component / "Cargo.toml")],
        capture_output=True, text=True,
    )
    target = Path(json.loads(info.stdout)["target_directory"])
    run(cargo + ["package", "--locked", "--manifest-path", str(component / "Cargo.toml")]
        + (["--allow-dirty"] if args.allow_dirty else []))
    archive = target / "package" / f"{name}-{version}.crate"
    with archive.open("rb") as package:
        digest = hashlib.file_digest(package, "sha256").hexdigest()
    # Cargo normalizes archive mtimes; never reuse another candidate's binaries.
    build_env = {**os.environ, "CARGO_TARGET_DIR":
                 str(target / "blad-package-verification" / digest)}
    with tempfile.TemporaryDirectory(prefix="kvist-blad-package-") as temporary:
        temporary = Path(temporary)
        with tarfile.open(archive) as package:
            prefix = f"{name}-{version}/"
            members = package.getmembers()
            require(all(member.name.startswith(prefix) for member in members),
                    "unexpected archive root")
            package.extractall(temporary, filter="data")
        source = temporary / f"{name}-{version}"
        for required in [
            "LICENSE", "README.md", "CHANGELOG.md", "docs/USAGE.md",
            "docs/SESSIONS.md", "docs/theme.example.json",
            "tests/fixtures/component_model.md", "ci/verify_package.py", "Cargo.lock",
        ]:
            require((source / required).is_file(), f"missing packaged {required}")
        require(not (source / "REQUIREMENTS.md").exists(), "unexpected intent payload")
        manifest = source / "Cargo.toml"
        normalized = tomllib.loads(manifest.read_text())
        require([binary["name"] for binary in normalized["bin"]] == ["blad"],
                "package must install only blad")
        require(normalized["lib"]["name"] == "blad", "unexpected library identity")
        lock = tomllib.loads((source / "Cargo.lock").read_text())
        peers = {"kvist", "sav", "galla", "skott"}
        require(not peers.intersection(item["name"] for item in lock["package"]),
                "peer component in standalone lockfile")
        document = temporary / "document.md"
        document.write_text("# Standalone\n\n- [x] done\n- [ ] todo\n")
        for mode, features in [
            ("default", ["--no-default-features"]),
            ("sessions", ["--features", "sessions"]),
        ]:
            run(cargo + ["test", "--locked", "--manifest-path", str(manifest)] + features,
                cwd=source, env=build_env)
            root = temporary / f"install-{mode}"
            run(cargo + ["install", "--locked", "--path", str(source),
                         "--root", str(root), "--bin", "blad"] + features,
                cwd=source, env=build_env)
            executable = root / "bin" / "blad"
            require([path.name for path in (root / "bin").iterdir()] == ["blad"],
                    "unexpected installed executable")
            installed_version = run([str(executable), "--version"],
                                    capture_output=True, text=True).stdout
            require(installed_version.strip() == f"blad {version}", "incorrect version")
            help_text = run([str(executable), "--help"],
                            capture_output=True, text=True).stdout
            require(("--log-dir" in help_text) == (mode == "sessions"),
                    "incorrect feature-specific CLI")
            output = run([str(executable), "--color", "always", "--dump", str(document)],
                         capture_output=True, env={**os.environ, "PATH": ""}).stdout
            require(b"\x1b" not in output and b"[x] done" in output and b"[ ] todo" in output,
                    "incorrect standalone rendering or task state")
            piped = run([str(executable)], input=document.read_bytes(),
                        capture_output=True, timeout=10).stdout
            require(piped == output, "stdin snapshot differs from plain file rendering")
            redirected = run([str(executable), str(document)],
                             capture_output=True, timeout=10).stdout
            require(redirected == output, "redirected output is not plain rendered text")
        print(f"Verified {archive}: standalone archive tests and release installations passed.")


if __name__ == "__main__":
    main()
