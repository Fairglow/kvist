<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Provider Discovery, Template, Probe, and Qualification (`sav`)

**Component:** `sav`
**Task:** `sa-07-compliance-review`
**Compared artifacts:**
- Observed record derived from `sav/src/setup.rs`, `sav/src/catalog.rs`,
  `sav/src/registry.rs`, `sav/src/direct_transport.rs`, `sav/src/supervisor.rs`,
  and `sav/src/main.rs`, cross-checked against `sav/IMPL.md`
- `sav/CONTRACT.md` §`Provided interfaces`, §`Behavioral guarantees`,
  §`Security and authority` (the "provider setup contract")
- `sav/DESIGN.md` §`Algorithms-and-decisions` (named task requirement)
- `sav/REQUIREMENTS.md` §`AR-REQ-SETUP` (supporting)
- `ROOT_CONTRACT.md` §`Change-and-compliance-rules`
- bounded test evidence (named test functions, listed below)
**Reviewer stance:** Independent compliance reviewer for the `sa-07` provider
qualification chain. Per the task instruction, the observed provider discovery,
template, probe, and qualification behavior is derived **from source and tests**
(not from the intent documents) and then compared against the provider setup
contract. This record is the retained, independent comparison for adopting
provider-specific profiles. It is not self-certified and establishes no
acceptance or sign-off.

## Review boundary and method

The `sa-07` chain is: `sa-07-write-tests` → `sa-07-implement-code` →
`sa-07-security-audit` → **`sa-07-compliance-review` (this record)**. The
implementer and the security auditor did not author it.

The task directs a **source-and-test-derived** compliance pass: first derive the
observable provider behavior from `src/` and `tests/` without relying on the
intent documents, then compare that observed record against the provider setup
contract (`sav/CONTRACT.md`) and the named `DESIGN.md` algorithms. `sav/IMPL.md`
is consulted only as a supporting cross-check of the same observed behavior,
not as the source of the record. This is a deliberate, task-directed method and
is recorded here so the boundary is explicit. It mirrors the method retained by
the immediately preceding `sa-06-compliance-review`.

**Read (derivation):** `sav/src/{setup,catalog,registry,direct_transport,
supervisor,main,lib}.rs` and the bounded test evidence below.
**Read (comparison):** `sav/CONTRACT.md`, `sav/DESIGN.md` §`Algorithms-and-
decisions`, `sav/REQUIREMENTS.md` §`AR-REQ-SETUP`, `ROOT_CONTRACT.md`, and
`sav/IMPL.md` (cross-check only).
**Scoping only (not evidence):** `sav/TODOS.yaml` `sa-07-*` entries (task
`id`/`title`/`description`/`context`).
**Not executed:** the reviewer ran no tests, builds, probes, or real provider
CLIs. The bounded suites are run automatically by Kvist in the verification
sandbox after this turn; no pass/fail count from that run is asserted here. No
real `copilot`, `gemini`, `llama-cli`, `llama-server`, or `ollama` process was
started, so no statement in this record certifies that a locally installed
provider's real CLI matches the recorded flags; that is precisely the
uncertainty the task names and is carried explicitly in O-5.

## Derived provider-qualification record (source + tests)

`sav` has **one canonical provider-qualification surface** wired to the CLI and
to hosts (`run_registry_setup` / `qualify_registry_profile`, operating the typed
`sav::registry::Profile`) and **one legacy command-template surface**
(`collect_profile*` / `verify_profile`, operating `sav::ModelProfile`). The
`sa-07` chain changed the canonical surface's discovery, templates, and probe
behavior. The legacy surface is the retained `sa-06` `M-1` and is covered here
only as to its provider-specific divergences.

### Canonical surface — `run_registry_setup` (`src/setup.rs:24`)

`sav setup [--registry PATH] [--force]` (`src/main.rs:759`) binds stdin/stdout
and calls `run_registry_setup`. Observed behavior:

- **Provider menu (6):** `1) llama-cli (external process)`, `2) llama-server
  (native HTTP)`, `3) Ollama (native HTTP)`, `4) Copilot (external agent)`,
  `5) Gemini (external agent)`, `6) Custom executable (external agent)`,
  default `[3]`. `ExternalProvider::Claude` is representable in the enum
  (`src/registry.rs`) but is **not** a wizard choice.
- **Discovery dispatch:** llama-server/Ollama take a base-URL prompt (default
  from `SetupOptions` or `provider.default_endpoint()`) then `select_discovered_model`
  over `CatalogProvider::LlamaServer`/`Ollama`; Copilot/Gemini first run the
  executable probe/fallback then `select_discovered_model` with
  `allow_host_discovery: true` hard-coded; llama-cli (1) and custom (6) take a
  manual file/wrapper path and run **no** catalog discovery.
- **Templates (typed `ExternalAgent` argv, `src/setup.rs:139-168`):** base is
  `["--prompt", "{prompt}"]` plus `["--model", <model>]` when a model is
  present. Per provider:
  - `llama-cli`: `+ ["--single-turn", "--simple-io", "--no-display-prompt",
    "--predict", "4096"]` (model = the manually supplied GGUF path).
  - `copilot`: `+ ["--silent", "--allow-all-tools", "--no-ask-user",
    "--reasoning-effort", "{reasoning_effort}"]`.
  - `gemini`: `+ ["--output-format", "text", "--approval-mode", "yolo",
    "--skip-trust"]`.
  - custom: no added flags, no model.
- **Qualification:** after building and re-validating the profile, it prints
  `Qualifying with the fixed prompt `Reply with exactly: OK`...` and calls
  `qualify_registry_profile(&profile, working_directory, true)`.

### Bounded discovery (`src/catalog.rs:212`)

- `validate_options` bounds discovery to a positive timeout ≤ 30 s
  (`MAX_DISCOVERY_TIMEOUT`) and 1..=1 MiB (`MAX_DISCOVERY_BYTES`); setup uses the
  tighter `setup_discovery_options` of 5 s / 64 KiB (`src/setup.rs:720`).
- **HTTP** (`src/catalog.rs:252`): Ollama reads `/api/tags`, llama-server reads
  `/v1/models`, through `get_bounded` (`src/direct_transport.rs:533`) which
  parses the endpoint via `split_authority`/`parse_port`, requiring an
  **explicit port** and a **numeric loopback** host (rejecting user-info,
  query, fragment, and non-loopback hosts). No prompt is sent.
- **ACP** (`src/catalog.rs:340`): requires `allow_host_discovery` else
  `HostDiscoveryNotAcknowledged`. Spawns the selected executable in a new
  process group with `--acp`, sends only `initialize`
  (`protocolVersion: 1`, empty client capabilities) and `session/new` (the
  setup working directory, empty `mcpServers`, no client filesystem/terminal/MCP
  bridges), correlates the numeric response IDs, rejects provider-to-client
  requests, and parses `models.availableModels` / `currentModelId`
  (`src/catalog.rs:466`). The process group is `SIGKILL`-terminated and reaped on
  every outcome. No prompt is sent.
- **Catalog invariants** (`ModelCatalog::new`, `src/catalog.rs`): at most 128
  unique models, duplicate IDs retain the first occurrence, an empty catalog is
  a failure, `current_model_id` is retained only when it is itself in the list
  (else `default_model_id()` = the first model). IDs are 1–256 printable ASCII
  bytes without braces; names/descriptions are bounded UTF-8 without control
  characters.

### Bounded advisory probe (`src/setup.rs:644,698`)

`select_cli_executable` prints `Checking `<name>` `--version`...` and runs
`probe_cli_version` (`--version`) under `SupervisionPolicy { idle_timeout: 10 s,
attempt_timeout: 10 s, detect_loops: false, max_retries: 0, max_output_bytes:
64 KiB }`. On a failed (non-cancelled) probe it prompts for a direct executable
or wrapper, resolves it as a regular non-link file (canonicalized), checks the
executable bit (`0o111`), and re-probes that exact path; a failed re-probe is an
actionable `ProfileSetup` error naming the probe and the path. Cancellation
propagates **without** requesting a fallback. The returned executable path is
independent of the provider identity, which is fixed by the menu selection
(`ExternalProvider::LlamaCli`/`Copilot`/`Gemini`/`CustomScript`).

### Qualification (`src/setup.rs:222`)

`SETUP_TEST_PROMPT = "Reply with exactly: OK"` (`src/setup.rs:18`) is the single
shared fixed prompt. `qualify_registry_profile` refuses to act unless
`host_execution_acknowledged` (else `HostExecutionNotAcknowledged`), re-validates
the profile, then:

- **Native** (llama-server/Ollama): one `ModelRequest` with the fixed prompt,
  no tools, `ToolChoice::None`, `max_output_tokens: Some(32)`, through
  `model.transport().complete(...)`.
- **External agent:** `run_supervised_capture` of `agent.command_spec(
  SETUP_TEST_PROMPT, &[], working_directory, None)` under `SupervisionPolicy {
  idle_timeout: 30 s, attempt_timeout: 300 s, detect_loops: true, max_retries: 0,
  max_output_bytes: 64 KiB }`. `command_spec` (`src/registry.rs:247`) renders the
  literal argv with one-time `{prompt}`/`{model}`/`{cwd}`/`{reasoning_effort}`
  substitution and `{context_files}` expansion (≤256 args, ≤2 MiB, no NUL).

The supervised process's stdin is `Stdio::null()` (`src/supervisor.rs:479`), so a
provider cannot consume the setup caller's stdin. Output is captured, never
forwarded as setup presentation.

### Gating and persistence (`src/setup.rs` ~200-220)

- Success prints `Qualification succeeded (not execution authorization).`
- `Error::Cancelled` returns immediately (cancellation is **not** force-bypassable).
- A non-cancelled failure with `--force` prints `Qualification failed: {error}`
  plus `--force saves this unqualified profile.` and persists.
- A non-cancelled failure without `--force` returns a `ProfileSetup` error
  (`qualification failed; profile not saved: {error}; --force permits explicit
  persistence`) and persists **nothing**.
- Persistence is `registry::upsert` (typed schema-one TOML) followed by
  `Registry::load().resolve(&name)`. There is no interactive
  `Save profile anyway?` prompt.

### Standalone `sav models` (`src/main.rs:446`)

`models --provider PROVIDER [--endpoint URL] [--executable PATH]
[--allow-host-discovery] [--json]` reuses `discover_models` without inference.
`--provider` accepts `ollama`/`llama-server`/`copilot`/`gemini` and returns an
explicit `UnsupportedCapability` for `llama-cli`/`custom-script` (no catalog, no
spawn). `--executable` is rejected for HTTP providers and `--endpoint` for ACP
providers. `allow_host_discovery` defaults to false (the flag is required for ACP
outside setup). Text output is one validated `model.id()` per line; JSON output
is one object `{format_version: 1, provider, current_model_id, models:[{id,name,
description?}]}` on stdout; failures use stderr and a nonzero status.

### Bounded test evidence (canonical surface)

`tests/cli.rs`: `setup_uses_fixed_prompt_and_refuses_failed_qualification`,
`setup_force_persists_profile_after_failed_qualification`,
`supervised_provider_cannot_consume_caller_stdin`,
`setup_persists_a_profile_and_run_can_select_it`,
`llama_cli_fallback_preserves_provider_and_uses_supported_flags`,
`installed_gemini_and_copilot_use_noninteractive_templates`,
`setup_uses_the_acp_current_model_as_the_numbered_default`,
`unusable_provider_fallback_is_rejected_before_persistence`,
`cancelling_conventional_probe_does_not_request_a_fallback`,
`interrupt_terminates_the_supervised_process_group`,
`models_http_text_and_json_outputs_are_deterministic`,
`models_reports_manual_providers_as_unsupported_without_spawning`,
`models_acp_requires_host_discovery_acknowledgement_before_spawn`,
`models_acp_lists_correlated_provider_ids`.
`tests/catalog.rs`: `canonical_catalog_is_bounded_deduplicated_and_serializes_as_
version_one`, `canonical_catalog_rejects_invalid_or_empty_models`,
`acp_catalog_rejects_empty_and_oversized_state_and_drops_inconsistent_current`,
`discovers_ollama_details_and_llama_server_ids_over_bounded_loopback_http`.
`tests/registry.rs`: `external_identity_is_structured_and_resolution_never_
executes`, `structured_argv_never_reinterprets_prompt_placeholders_or_shell_syntax`,
`rejects_unknown_fields_credentials_bounds_and_retired_command_schema`.

Key evidence points: the fixed prompt is written to the captured prompt file and
is exactly `Reply with exactly: OK`, and no `Test prompt`/`full host
permissions?`/`Save profile anyway?` prompt appears; a failed qualification
leaves no registry file while `--force` persists it; the ACP `currentModelId`
becomes the numbered default when present; a fallback executable keeps
`ExternalProvider::LlamaCli` (and the supported `--single-turn`/`--simple-io`/
`--no-display-prompt`/`4096` flags, no `--context`/`--format`); Copilot and
Gemini templates carry `--silent`/`--allow-all-tools`/`--no-ask-user`/
`--reasoning-effort {reasoning_effort}` and `--output-format text`/`yolo`/
`--skip-trust` respectively; an unusable fallback is rejected with a `version
probe` diagnostic before any persistence; cancelling a hung `--version` probe
reports `cancelled` without requesting a fallback; and SIGINT terminates the
whole supervised process group.

### Legacy command-template surface (`src/setup.rs:285,392,405`)

`collect_profile_with_options` / `verify_profile` / `run_setup_wizard*` operate
`sav::ModelProfile` and persist to the retired `sav/config.toml` store. Their
provider templates are **command strings** (llama-server → `curl ... /v1/
chat/completions`, Ollama → `env OLLAMA_HOST=... ollama run ...`), they use the
`render_command` placeholder vocabulary, and their Gemini menu maps selection 5
to the provider string `gemini-cli` (while probing the conventional name
`gemini`). They are exercised by `tests/setup.rs` and `tests/profiles.rs` and
are wired to no CLI subcommand. See `sa-06` `M-1` and the sa-07-specific
instances O-1/O-2 below.

## Provider-setup contract classification (`sav/CONTRACT.md`)

Statuses use the runbook terms. `C` = `sav/CONTRACT.md` locator, `D` =
`sav/DESIGN.md`, `R` = `sav/REQUIREMENTS.md`; evidence is the named test or
source locator above.

### P-1 — Setup always tests the generated profile with the fixed prompt; it
neither asks for a test prompt nor a second host-execution confirmation; output
is captured, not forwarded; only status is reported. **Compliant.**

`C` §`Behavioral guarantees`. Observed: the single `SETUP_TEST_PROMPT`
(`src/setup.rs:18`) is used for both qualification paths; `setup_uses_fixed_
prompt_and_refuses_failed_qualification` asserts the captured prompt is exactly
`Reply with exactly: OK` and that no `Test prompt`/`full host permissions?`
prompt is emitted; `qualify_registry_profile`/`verify_profile` capture output.

### P-2 — A failed qualification does not persist unless `--force`; there is no
interactive save-after-failure prompt; cancellation is not bypassable.
**Compliant.**

`C` §`Behavioral guarantees`. Observed: the non-`--force` failure returns before
`upsert` (`qualification failed; profile not saved: ...`); `--force` is a flag,
not a prompt (`setup_force_persists_profile_after_failed_qualification`);
`Error::Cancelled` returns immediately. `sa-06` `G-2` covers the CLI wrapper.

### P-3 — Ollama/llama-server discovery uses `/api/tags`/`/v1/models` over
numeric-loopback HTTP endpoints with explicit ports, bounded in time and bytes.
**Compliant.**

`C` §`Behavioral guarantees`. Observed: `discover_http` selects those paths and
`get_bounded`→`split_authority`/`parse_port` require an explicit port and a
numeric loopback host; timeouts/byte bounds are enforced. Evidence:
`discovers_ollama_details_and_llama_server_ids_over_bounded_loopback_http`,
`rejects_non_loopback_discovery_urls_without_starting_a_provider`.

### P-4 — Copilot/Gemini discovery starts the selected executable in ACP mode,
initializes protocol version 1, creates a no-prompt session in the setup working
directory with an empty MCP-server list and no client filesystem/terminal/MCP
bridges; the provider remains an opaque host-authority process and setup
consumes `models.availableModels` from the correlated `session/new` response.
ACP host acknowledgement is required outside setup. **Compliant.**

`C` §`Behavioral guarantees`, §`Security and authority`. Observed: `discover_acp`
sends only `initialize` + `session/new` (empty `mcpServers`, no client
capabilities), correlates numeric IDs, reads `availableModels`, and terminates +
reaps the process group; `models` requires `--allow-host-discovery`
(`models_acp_requires_host_discovery_acknowledgement_before_spawn`) while setup
treats the setup action as the acknowledgement (`allow_host_discovery: true`
hard-coded). Evidence: `setup_uses_the_acp_current_model_as_the_numbered_default`,
`models_acp_lists_correlated_provider_ids`. The authority breadth of running a
real ACP session under the single setup acknowledgement is carried in O-4/O-5.

### P-5 — Catalog: first valid occurrence per model ID, current model retained
only when it is in the list else the first model is the default, empty/wholly
invalid catalog is a discovery failure; setup appends `Other model ID...` after
every list and requests free-form input only after that entry; discovery failure
offers `auto` (Copilot/Gemini), `default` (llama-server), or `llama3.1:8b`
(Ollama); Gemini's `auto` is stored as `--model auto`. **Compliant.**

`C` §`Behavioral guarantees`. Observed: `ModelCatalog::new` dedups, drops an
absent current, and fails on empty; `select_discovered_model` appends the custom
choice and prompts free-form only when it is selected, with the named fallbacks.
Evidence: `canonical_catalog_is_bounded_deduplicated_and_serializes_as_version_
one`, `acp_catalog_rejects_empty_and_oversized_state_and_drops_inconsistent_
current`, `llama_server_keeps_explicit_default_when_discovery_is_unavailable`,
`installed_gemini_and_copilot_use_noninteractive_templates` (Gemini `auto`
present in argv).

### P-6 — `sav models` exposes the same catalogs without inference; text is one
validated ID per line, JSON is one `format_version: 1` object with nullable
`current_model_id` and ordered `{id,name,description?}` models; IDs are 1–256
printable ASCII excluding braces; success to stdout, failure to stderr + nonzero
status; `--executable` only for ACP and `--endpoint` only for HTTP. **Compliant.**

`C` §`Provided interfaces`, §`Behavioral guarantees`. Observed: `src/main.rs:446`
implements exactly this, including the `--executable`/`--endpoint` cross-rejection
and the `UnsupportedCapability` for `llama-cli`/`custom-script`. Evidence:
`models_http_text_and_json_outputs_are_deterministic`,
`models_reports_manual_providers_as_unsupported_without_spawning`.

### P-7 — Provider input matrix defaults: Ollama `http://127.0.0.1:11434` /
llama-server `http://127.0.0.1:9931` (first advertised model); Copilot default
executable `copilot` and Gemini default executable `gemini` (advertised current
model). **Compliant.**

`C` §`Behavioral guarantees` (matrix). Observed: `CatalogProvider::
default_endpoint`/`default_executable` and the `run_registry_setup` defaults
match; the ACP advertised current becomes the numbered default (P-5). Evidence:
`setup_uses_the_acp_current_model_as_the_numbered_default`,
`ollama_default_materializes_the_selected_endpoint` (legacy, cross-check).

### P-8 — `llama-cli` and custom wrappers return an explicit unsupported-catalog
error without spawning discovery; their setup paths request an explicit file or
wrapper (manual, no catalog). **Compliant.**

`C` §`Behavioral guarantees`. Observed: `CatalogProviderArgument::catalog_
provider` returns `UnsupportedCapability` for both; `sav models` spawns nothing
(the marker file is never created); setup requests the GGUF path (llama-cli) or
the wrapper path (custom). Evidence: `models_reports_manual_providers_as_
unsupported_without_spawning`, `llama_cli_fallback_preserves_provider_and_uses_
supported_flags`.

### P-9 — Bounded advisory probe; maintained noninteractive provider templates;
provider identity preserved across the fallback path; null provider stdin;
process-group cleanup on interrupt; actionable pre-save failures. **Compliant.**

`C` §`Security and authority` (`Endpoint probes are advisory and bounded`),
§`Behavioral guarantees`. Observed: `--version` probe bounded to 10 s/10 s/64 KiB/
0-retry; templates carry the noninteractive flag sets (P-1/O-5); the fallback
executable path is independent of the fixed `ExternalProvider` identity
(`llama_cli_fallback_preserves_provider_and_uses_supported_flags`); the supervised
stdin is `Stdio::null()` (`supervised_provider_cannot_consume_caller_stdin`);
SIGINT reaps the whole group (`interrupt_terminates_the_supervised_process_group`);
probe/qualification/model-selection failures carry actionable `ProfileSetup`
reasons (`unusable_provider_fallback_is_rejected_before_persistence` names the
`version probe`).

### P-10 — Setup qualification treats the explicit setup action as
acknowledgement for the bounded provider-discovery commands and the exact
generated test command; this is not isolation and does not authorize any later
`run`. **Compliant.**

`C` §`Security and authority`. Observed: `qualify_registry_profile` is the only
spawn point under the setup acknowledgement; the completion is labeled `not
execution authorization`; the profile is stored inertly and later execution
requires a separate host acknowledgement (sa-06 `G-4`/`G-7`). The breadth of the
authority the stored template will exercise on a later run is the O-5 concern.

## Discrepancies and observations (retained for human arbitration)

### M-1 (HIGH, cross-reference, not re-opened) — an exported, tested, but
unwired second command-template setup/store coexists with the canonical
registry.

Established and retained by `sa-06-compliance-review` (`tests/
compliance_review_model_setup.md` M-1). The `sa-07` chain does not change that
finding; it adds the two provider-specific instances below (O-1, O-2). This is
recorded here only so the provider-qualification comparison is complete and the
arbitration list for the dual-surface risk is not split across reviews.

### O-1 (LOW, observation) — the legacy surface labels Gemini `gemini-cli` while
the canonical surface and contract name it `gemini`.

- *Locator:* `sav/src/setup.rs:306,552` (legacy selection 5 → provider string
  `"gemini-cli"`, probing conventional name `gemini`) vs. `sav/src/setup.rs:93`
  + `sav/src/registry.rs` `ExternalProvider::Gemini` (`as_str = "gemini"`) and
  the contract input matrix (`gemini`, default executable `gemini`).
- *Observed:* a profile persisted by the **canonical** surface stores
  `provider = "gemini"` (the contract name); the **legacy** surface stores the
  free-form string `gemini-cli` in the retired store. `rejects_unknown_fields_
  credentials_bounds_and_retired_command_schema` confirms the registry schema
  does not accept the retired command schema, so the two labels are not
  interchangeable.
- *Effect:* bounded to the retired store (no host reads it, per `sa-06` M-1/N-6),
  but it is a concrete instance of the "provider identity" divergence. Retain
  with M-1.

### O-2 (LOW, observation) — legacy Copilot/Gemini templates place `--model`
after the provider flags; the canonical argv places it immediately after
`--prompt`.

- *Locator:* `sav/src/setup.rs:544` (legacy Copilot `... --no-ask-user
  --reasoning-effort '{reasoning_effort}' --model {}`), `sav/src/setup.rs:577`
  (legacy Gemini `... --skip-trust --model {}`) vs. `sav/src/setup.rs:139-168`
  (canonical `--prompt {prompt} --model {} <provider flags...>`).
- *Observed:* the two surfaces emit the same flags in a different argv order.
- *Effect:* bounded to the retired store. Retain with M-1 / `sa-06` N-1.

### O-3 (LOW, observation) — setup discovery is fixed at 5 s / 64 KiB while
`sav models` is user-adjustable up to the 1 MiB ceiling.

- *Locator:* `sav/src/setup.rs:720` (`setup_discovery_options`: 5 s / 64 KiB) vs.
  `sav/src/main.rs` `ModelsArguments` (`--timeout` default 5, `--max-response-
  bytes` default 65 536, validated up to 1 MiB by `catalog::validate_options`).
- *Observed:* the interactive setup path is pinned to the tighter bound; the
  standalone `models` command lets a caller widen the response bound. Both are
  within the contract's "bounded" requirement and the 1 MiB ceiling, so this is
  not a violation.
- *Effect:* a behavior the contract does not pin; recorded so the two
  surfaces' discovery budgets are explicit for arbitration. No action strictly
  required.

### O-4 (LOW, compliant disclosure) — the ACP discovery authority rides on the
single setup acknowledgement.

- *Locator:* `sav/src/setup.rs:122` (`allow_host_discovery: true` hard-coded for
  Copilot/Gemini); `sav/src/catalog.rs:340` (ACP spawn under inherited host
  authority); `sav/CONTRACT.md` §`Security and authority`.
- *Observed:* outside setup, ACP discovery requires `--allow-host-discovery`;
  inside setup the interactive setup action is the acknowledgement, so one
  operator action authorizes a full no-prompt ACP session (which may initialize
  the selected CLI's own configured credentials/filesystem/network) plus the
  qualification command.
- *Effect:* this matches the contract exactly (P-4/P-10) and is an explicit
  contract disclosure, not a defect. It is confirmed here as compliant; the
  residual authority-breadth question is carried by O-5.

### O-5 (MEDIUM, retained) — the maintained templates embed blanket
authority-escalation flags that are test-asserted but not contract-pinned; the
authority breadth is an approved design deferral, and the exact flag set's
portability cannot be certified from the contract.

- *Locator:* `sav/src/setup.rs:155-156` (Copilot `--allow-all-tools
  --no-ask-user`), `sav/src/setup.rs:165-167` (Gemini `--approval-mode yolo
  --skip-trust`); `sav/CONTRACT.md` §`Behavioral guarantees` (silent on the
  specific flags); `sav/DESIGN.md` §`Algorithms-and-decisions` ("Restricting
  qualification to a narrower tool/filesystem authority is deferred with the
  broader execution backend work"); evidence `installed_gemini_and_copilot_use_
  noninteractive_templates`.
- *Intended (contract):* "maintained provider templates" with noninteractive
  arguments, and a fixed-prompt qualification that does not authorize a later
  `run`. The contract names the discovery defaults and the `--model auto`
  storage but does **not** name the permission flags or assign them a
  maintenance/version policy.
- *Observed:* the canonical templates are the most-permissive noninteractive
  modes of each provider CLI (`--allow-all-tools --no-ask-user`; `--approval-mode
  yolo --skip-trust`). Because the qualification template **is** the stored
  profile command, these flags also ride along into every later execution of the
  profile by any host that separately acknowledges execution. The flags are held
  only by the test assertions and by the DESIGN's explicit deferral of authority
  restriction; they are not pinned in the contract.
- *Why it matters:* this is the concrete substance of the task's context —
  "the implementer cannot certify that locally observed CLI behavior has been
  represented accurately or portably." These flags were derived from locally
  installed provider CLIs; a provider version that renames, removes, or
  re-defaults them would make the stored template non-portable, and no contract
  clause would hold that behavior. The authority *breadth* is an approved design
  deferral, so the item is not a behavioral defect; it is a contract-coverage
  gap retained for explicit arbitration.
- *Arbitration options:* (a) pin the exact permission flags (and a
  version/maintenance policy for when a provider CLI changes them) in
  `CONTRACT.md`, naming that the flags are max-permission and version-sensitive;
  (b) record an explicit approved deferral that the `DESIGN.md` deferral of
  authority restriction also covers the flag set and its portability, with a
  named condition for when it must be pinned; or (c) re-scope qualification to a
  narrower (non-`yolo`/non-`allow-all-tools`) probe command so the stored
  template does not carry blanket authority into later runs.

## Discrepancy summary

| ID  | Clause / finding                                                    | Status                        | Severity |
|-----|---------------------------------------------------------------------|-------------------------------|----------|
| P-1 | Fixed-prompt qualification; no test prompt / second confirmation    | compliant                     | none     |
| P-2 | Failed qualification blocks persistence; `--force` non-interactive  | compliant                     | none     |
| P-3 | Ollama/llama-server loopback-HTTP discovery with explicit ports     | compliant                     | none     |
| P-4 | Copilot/Gemini ACP no-prompt discovery; ack required outside setup  | compliant                     | none     |
| P-5 | Catalog invariants; `Other model ID...`; fallbacks; `--model auto`  | compliant                     | none     |
| P-6 | `sav models` text/JSON output; provider/flag cross-rejection        | compliant                     | none     |
| P-7 | Provider input matrix defaults (endpoints, executables, models)     | compliant                     | none     |
| P-8 | llama-cli/custom unsupported-catalog; manual file/wrapper           | compliant                     | none     |
| P-9 | Bounded probe; templates; identity preserved; null stdin; cleanup   | compliant                     | none     |
| P-10| Setup action = bounded acknowledgement; no later `run` authorization| compliant                     | none     |
| M-1 | Dual setup/store (registry + retired command-template)              | mismatched (sa-06)            | high     |
| O-1 | Legacy `gemini-cli` label vs canonical/contract `gemini`            | observation (M-1 instance)    | low      |
| O-2 | Legacy argv-order divergence for `--model`                          | observation (M-1 instance)    | low      |
| O-3 | Setup discovery fixed 5 s/64 KiB vs `models` up to 1 MiB            | observation                   | low      |
| O-4 | ACP discovery authority under the single setup acknowledgement      | compliant disclosure          | low      |
| O-5 | Blanket authority flags test-asserted, not contract-pinned; portability | retained (deferral/underspecified) | medium   |

**Total hard behavioral mismatches against the provider setup contract:** 0.
**Contract-coverage items retained for arbitration:** 1 new (O-5, medium) plus
the cross-referenced `sa-06` `M-1` (high), with O-1/O-2 as its provider-specific
instances and O-3/O-4 as low observations (O-4 a compliant disclosure).

## Conclusion

Every provider-qualification requirement of the provider setup contract was
classified: the `sa-07` canonical surface is **compliant on all ten clause
comparisons** (P-1..P-10). Discovery is bounded and provider-appropriate
(loopback-HTTP for Ollama/llama-server, no-prompt ACP for Copilot/Gemini,
unsupported-catalog for llama-cli/custom); the maintained templates produce
noninteractive, provider-preserving argv (identity is retained across the
fallback path); the probe is bounded and advisory; the fixed-prompt
qualification is mandatory, non-interactive, cancellation-safe, and
`--force`-without-an-interactive-bypass; and the setup action is a bounded
acknowledgement that does not authorize a later `run`.

No observed behavior **contradicts** the provider setup contract. The retained
items are contract-coverage gaps, not behavioral defects: (1) the `sa-06`
dual-surface `M-1`, now with two provider-specific instances (O-1 `gemini-cli`
label, O-2 argv order) that keep the two setup stores' provider identities and
flag orderings divergent; (2) O-5, the headline item — the Copilot/Gemini
templates embed blanket authority-escalation flags (`--allow-all-tools
--no-ask-user`, `--approval-mode yolo --skip-trust`) that are test-asserted,
max-permission, carried into later execution, and version-sensitive, with the
authority breadth an explicit `DESIGN.md` deferral but the exact flag set
unpinned in the `CONTRACT.md`. That is precisely the "cannot certify locally
observed CLI behavior has been represented accurately or portably" gap the task
names, and it is recorded here for explicit arbitration (pin the flags + a
maintenance policy, or extend the deferral to cover them, or re-scope the probe
to a narrower authority).

This record is the independent gate disposition for the `sa-07` provider
qualification chain. It is not self-certified, establishes no clause-level
certification beyond the record above, and does not constitute acceptance or
sign-off. Formal sign-off is blocked while `M-1` and `O-5` remain unarbitrated;
acceptance, if required, remains subject to the separate review
receipt/acknowledgement or a reasoned exception per the change-and-compliance
rule.
