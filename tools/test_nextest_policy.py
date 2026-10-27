"""Repository-only scheduling checks; not part of standalone Blad packaging."""

import pathlib
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
        with (ROOT / "tools/nextest.toml").open("rb") as source:
            self.profiles = tomllib.load(source)["profile"]

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


if __name__ == "__main__":
    unittest.main()
