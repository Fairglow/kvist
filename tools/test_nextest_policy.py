"""Repository-only scheduling checks; not part of standalone Blad packaging."""

import pathlib
import shlex
import subprocess
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parent.parent
BLAD_FILTER = """package(=kvist-blad) & (
    binary(=diagrams) |
    binary(=mermaid_backend) |
    test(=markdown::tests::test_mermaid_block_is_processed) |
    test(/^mermaid::budget_tests::/) |
    (binary(=render_options) & test(=color_depth_never_changes_visible_geometry)) |
    (binary(=policies) & test(=rendering_needs_no_runtime_tools_and_does_not_consume_stdin)) |
    (binary(=integration) & test(=every_session_route_uses_the_same_escape_free_dump_policy)) |
    (binary(=viewer) & test(=ascii_diagram_overflow_remains_scrollable_and_canonical_on_resize))
)"""


def normalized(expression):
    return " ".join(expression.split())


class NextestPolicyTests(unittest.TestCase):
    def setUp(self):
        with (ROOT / ".config/nextest.toml").open("rb") as source:
            self.profiles = tomllib.load(source)["profile"]

    def test_policy_is_at_nextests_default_discovery_path(self):
        config = ROOT / ".config/nextest.toml"
        self.assertTrue(config.is_file(), "ordinary nextest runs must discover the policy")
        with config.open("rb") as source:
            self.assertEqual(tomllib.load(source)["profile"], self.profiles)
        self.assertFalse((ROOT / "tools/nextest.toml").exists(),
                         "keep one canonical scheduling policy, not divergent copies")

    def test_only_the_shared_policy_is_exposed_by_git_ignore_rules(self):
        for path, expected in [
            (".config/nextest.toml", 1),
            (".config/local-settings.toml", 0),
            (".config/private/settings.toml", 0),
        ]:
            with self.subTest(path=path):
                result = subprocess.run(
                    ["git", "check-ignore", "--no-index", "--quiet", path],
                    cwd=ROOT, capture_output=True, text=True, check=False,
                )
                self.assertEqual(result.returncode, expected, result.stderr)

    def test_repository_entrypoints_use_standard_discovery(self):
        for path in ["justfile", ".github/workflows/rust.yml"]:
            with self.subTest(path=path):
                source = (ROOT / path).read_text().replace("\\\n", " ")
                commands = [line[line.index("cargo nextest run"):]
                            for line in source.splitlines() if "cargo nextest run" in line]
                self.assertEqual(len(commands), 2)
                for command in commands:
                    arguments = shlex.split(command)
                    self.assertFalse(
                        any(arg == "--config-file" or arg.startswith("--config-file=")
                            for arg in arguments),
                        "repository entrypoints must exercise default discovery",
                    )

    def test_native_mermaid_fixture_scope_reserves_the_whole_pool(self):
        rules = self.profiles["default"]["overrides"]
        matches = [
            rule
            for rule in rules
            if normalized(rule["filter"]) == normalized(BLAD_FILTER)
        ]
        self.assertEqual(
            len(matches), 1,
            "all native Mermaid fixtures, including color geometry, need one scoped override",
        )
        self.assertEqual(matches[0]["threads-required"], "num-test-threads")
        # A higher-priority broad override must not shadow the fixture policy.
        self.assertEqual(
            [rule["filter"] for rule in rules[:rules.index(matches[0])]],
            ["package(=skott) & binary(=rust_build_environment) & "
             "test(=native_repository_tests_execute_inside_the_workspace_sandbox)"],
        )

    def test_ci_inherits_default_reservations_without_overriding_them(self):
        self.assertEqual(self.profiles["ci"].get("overrides", []), [])
        self.assertNotIn("slow-timeout", self.profiles["ci"])
        self.assertEqual(self.profiles["default"]["slow-timeout"],
                         {"period": "30s", "terminate-after": 10})

    def test_exclusive_trials_precede_long_parallel_inventory_checks(self):
        rules = self.profiles["default"]["overrides"]
        exclusive = [rule for rule in rules
                     if rule.get("threads-required") == "num-test-threads"]
        self.assertEqual(len(exclusive), 2)
        self.assertTrue(all(rule.get("priority") == 100 for rule in exclusive))
        inventories = [rule for rule in rules if rule.get("priority") == 50]
        self.assertEqual(len(inventories), 1)
        self.assertEqual(
            normalized(inventories[0]["filter"]),
            normalized("""(package(=skott) & binary(=rustup_inventory)) |
                (package(=kvist) & binary(=sandbox_toolchain) &
                 test(=rust_authoring_has_host_inventory_private_defaults_and_offline_builds))"""),
        )
        self.assertNotIn("threads-required", inventories[0])


if __name__ == "__main__":
    unittest.main()
