#!/usr/bin/env bash
set -euo pipefail

if [[ $(uname -s) != Linux || -e /.galla-sandbox || -L /.galla-sandbox ]]; then
    printf '%s\n' 'Galla host tests require outer Linux with working Bubblewrap; they cannot run inside Galla.' >&2
    exit 1
fi

printf 'Galla host lane: %s\n' "$(uname -sr)"
cargo --version
status=0
cargo test --offline --locked --no-fail-fast -p kvist-galla \
    --test conformance --test interactive_exec --test probe_and_backend \
    --test resource_monitoring --test scaffold --test schema_and_identity \
    --test session_mode --test toolchain_e2e -- \
    --ignored --test-threads=1 --nocapture || status=$?
cargo test --offline --locked -p kvist-galla --test native_cache \
    native_quality_runs_rust_tools_on_a_read_only_workspace_member -- \
    --ignored --exact --nocapture || status=$?
exit "$status"
