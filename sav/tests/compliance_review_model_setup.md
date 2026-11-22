<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Model Setup Configuration Boundary (`sav`)

**Component:** `sav`
**Task:** `sa-06-compliance-review`
**Compared artifacts:**
- Observed record derived from `sav/src/setup.rs`, `sav/src/registry.rs`,
  `sav/src/profile.rs`, `sav/src/command.rs`, `sav/src/catalog.rs`,
  `sav/src/main.rs`, and `sav/src/lib.rs`, cross-checked against `sav/IMPL.md`
  "Shared registry, setup, and pinned resolution"
- `sav/REQUIREMENTS.md` §`AR-REQ-SETUP`, §`AR-REQ-PROFILES`,
  §`Shared registry configuration acceptance (ADR 0015)`
- `sav/CONTRACT.md` §`Boundary and ownership`,
  §`Shared registry configuration (ADR 0015)`, §`Provided interfaces`,
  §`Data and schemas`, §`Behavioral guarantees`
- Kvist-binding contract: `maerg/CONTRACT.md` §`Shared registry configuration
  (ADR 0015)` (nearest consumer) and `maerg/src/wizard.rs`
- `ROOT_CONTRACT.md` §`Change and compliance rules`
- bounded test evidence (named test functions, listed below)

**Reviewer stance:** Independent compliance reviewer for the `sa-06` model-setup
chain. Per the task instruction, the observed setup behavior is derived **from
source and tests** (not from the intent documents) and then compared against the
generic-profile contract (`sav/CONTRACT.md`) and the Kvist-binding contract
(`maerg/CONTRACT.md`). This record is the retained, independent comparison for
the extracted configuration boundary. The implementer could not certify reuse
without risking Kvist-workflow leakage; this review is the record that either
confirms or refutes that risk. It is not self-certified and establishes no
acceptance or sign-off.

## Review boundary and method

The `sa-06` chain is: `sa-06-write-tests` → `sa-06-implement-code` →
`sa-06-security-audit` → **`sa-06-compliance-review` (this record)**. The
implementer and the security auditor did not author it.

The task directs a **source-and-test-derived** compliance pass: first derive the
observable setup behavior from `src/` and `tests/` without relying on the intent
documents, then compare that observed record against the generic-profile and
Kvist-binding contracts. `sav/IMPL.md` is consulted only as a supporting
cross-check of the same observed behavior, not as the source of the record.
This is a deliberate, task-directed method and is recorded here so the boundary
is explicit.

**Read (derivation):** `sav/src/{setup,registry,profile,command,catalog,main,
lib}.rs`, and the bounded test evidence below.
**Read (comparison):** `sav/CONTRACT.md`, `sav/REQUIREMENTS.md`,
`maerg/CONTRACT.md`, `maerg/src/wizard.rs`, `ROOT_CONTRACT.md`, and
`sav/IMPL.md` (cross-check only).
**Not executed:** the reviewer ran no tests, builds, or probes. The bounded
suites are run automatically by Kvist in the verification sandbox; no pass/fail
count from that run is asserted here.

## Derived setup-behavior record (source + tests)

`sav` exposes **two parallel model-setup surfaces**. This is the central
observed fact of the record.

### Surface 1 — canonical shared-registry setup (wired to the CLI and hosts)

`run_registry_setup` (`src/setup.rs:24`) and `qualify_registry_profile`
(`src/setup.rs:222`) operate the typed `sav::registry::Profile` and persist
through `sav::registry::upsert`. `sav setup [--registry PATH] [--force]`
(`src/main.rs`) binds stdin/stdout and calls `run_registry_setup`.

Observed behavior:

- **Provider menu:** `1) llama-cli (external process)`, `2) llama-server
  (native HTTP)`, `3) Ollama (native HTTP)`, `4) Copilot (external agent)`,
  `5) Gemini (external agent)`, `6) Custom executable (external agent)`, default
  `[3]`. `ExternalProvider::Claude` is representable in the enum but is **not**
  a wizard choice.
- **Discovery:** llama-server/Ollama use the shared bounded HTTP catalogs
  (`/v1/models`, `/api/tags`); Copilot/Gemini use the ACP `--acp`
  `session/new` catalog with `allow_host_discovery: true` hard-coded (the setup
  action is the discovery acknowledgement); llama-cli and custom are file-backed
  (no catalog) and take a manual path. Numbered model choices are offered with a
  final `Other model ID...` custom choice. The manual ID must be 1–256
  printable-ASCII bytes with no braces or quotes (`validate_model_id`).
- **Qualification:** one fixed, minimal, nonblank prompt —
  `SETUP_TEST_PROMPT = "Reply with exactly: OK"` — is used for both surfaces.
  Native backends send it through `model.transport()` with tools disabled and
  32 reserved output tokens; external agents run
  `run_supervised_capture` with a 30 s idle / 300 s attempt / 64 KiB / 0-retry
  policy and `detect_loops`. Output is captured, not forwarded as setup
  presentation.
- **Host acknowledgement:** `qualify_registry_profile` refuses to spawn unless
  `host_execution_acknowledged` is `true`; the wizard passes `true` (the setup
  action is the acknowledgement). There is no interactive re-confirmation prompt
  and no interactive save-confirmation.
- **Failure / `--force`:** a failed qualification returns an error before
  persistence unless `--force` is set, in which case the profile is persisted
  with a warning. Cancellation is **not** bypassable by `--force`.
- **Persistence:** `registry::upsert` → schema-one TOML, 64 KiB / 128-profile
  bounds, unknown-field rejection, inline-credential refusal, symlink/regularity
  checks, no-clobber first create, and validate-before/after synchronized atomic
  replacement (`profile::persist_configuration`).

Bounded evidence (canonical surface, `tests/cli.rs` and `tests/registry.rs`):
`setup_uses_fixed_prompt_and_refuses_failed_qualification`,
`setup_force_persists_profile_after_failed_qualification`,
`setup_persists_a_profile_and_run_can_select_it`,
`llama_cli_fallback_preserves_provider_and_uses_supported_flags`,
`installed_gemini_and_copilot_use_noninteractive_templates`,
`setup_uses_the_acp_current_model_as_the_numbered_default`,
`unusable_provider_fallback_is_rejected_before_persistence`,
`cancelling_conventional_probe_does_not_request_a_fallback`,
`registry_resolution_is_ordered_pinned_and_does_not_probe`,
`external_identity_is_structured_and_resolution_never_executes`,
`registry_upsert_roundtrips_and_refuses_symlinks`,
`disabled_profiles_and_invalid_present_sources_fail_closed`,
`structured_argv_never_reinterprets_prompt_placeholders_or_shell_syntax`,
`rejects_inline_credentials_and_independent_registry_bounds`,
`explicit_source_is_required_and_whole_profiles_override_not_fields`, and
(external zero-retry) `external_profiles_reject_positive_retries_before_spawning`
(`tests/registry_cli_policy.rs`).

### Surface 2 — legacy command-template wizard (exported, tested, unwired)

`collect_profile` / `collect_profile_with_options` (`src/setup.rs:276,285`),
`run_setup_wizard` / `run_setup_wizard_with_options` (`src/setup.rs:347,363`),
and `verify_profile` (`src/setup.rs:392`) operate the `sav::ModelProfile`
(`name`/`provider`/`command`) and persist through `profile::upsert_profile` to
the `sav/config.toml` array-of-tables store (`default_profile_config_path`).
All of these are re-exported from `sav/src/lib.rs` and exercised by
`tests/setup.rs` and `tests/profiles.rs`.

Observed behavior:

- **Provider menu:** `1) Local llama-cli`, `2) Local llama-server
  (default 127.0.0.1:9931)`, `3) Ollama (default 127.0.0.1:11434)`,
  `4) Copilot (CLI wrapper)`, `5) Gemini (CLI wrapper)`, `6) Custom Wrapper
  Script`, default `[3]`.
- **Storage model:** HTTP providers are materialized as **command strings**
  (llama-server → `curl ... /v1/chat/completions`; Ollama →
  `env OLLAMA_HOST=... ollama run <model> '{prompt}'`), and the command template
  uses the `render_command` placeholder vocabulary
  (`{prompt}`, `{prompt_json}`, `{context_files}`, `{target_directory}`,
  `{reasoning_effort}`).
- **Verification:** `verify_profile` renders the stored command through
  `render_command` and runs it under the same bounded supervision policy, gated
  on `host_execution_acknowledged`.
- **CLI wiring:** **none.** `sav setup` calls `run_registry_setup`, not
  `run_setup_wizard`. `sav run` and `sav model` resolve profiles only through
  `sav::registry::RegistryPaths` (`src/main.rs` `registry_paths`).
- **Host wiring:** **none.** Neither `maerg` nor `skott` references
  `collect_profile`, `run_setup_wizard`, `verify_profile`, `upsert_profile`,
  `load_profiles`, `load_profile`, or `default_profile_config_path` (bounded
  search of `maerg/src` and `skott/src` returned zero references; the only
  sav setup entry point a host uses is `sav::run_registry_setup`,
  `maerg/src/wizard.rs`).

Bounded evidence (legacy surface, `tests/setup.rs` and `tests/profiles.rs`):
`rejects_non_loopback_discovery_urls_without_starting_a_provider`,
`verification_refuses_before_spawning_without_host_acknowledgement`,
`llama_server_default_json_encodes_the_rendered_prompt`,
`llama_server_lists_models_and_keeps_profile_name_separate`,
`llama_server_keeps_explicit_default_when_discovery_is_unavailable`,
`llama_server_accepts_an_advertised_numeric_model_id`,
`llama_server_rejects_zero_for_forced_list_selection`,
`llama_server_rejects_template_tokens_in_manual_model_ids`,
`rejects_quoted_manual_model_ids_that_would_escape_the_template`,
`ollama_default_materializes_the_selected_endpoint`,
`update_preserves_comments_unrelated_values_and_profiles`,
`rejects_symlinked_configuration_on_load`, `rejects_symlinked_configuration_on_upsert`.

### Store separation (test-confirmed)

The two stores are disjoint and the registry is authoritative for the CLI:

- `retired_profile_store_is_not_read_when_registry_is_absent`
  (`tests/cli.rs`): a legacy `supervised-agent/config.toml` profile is **not**
  read by `sav run --profile`; the run fails `not found`.
- `canonical_profile_store_takes_precedence_over_legacy_store`
  (`tests/cli.rs`): a malformed canonical `sav/registry.toml` is the one that
  errors (naming the registry path), not the legacy store.
- `invalid_xdg_config_home_falls_back_to_absolute_home` (`tests/cli.rs`): the
  canonical store resolves through `XDG_CONFIG_HOME/sav/registry.toml` or
  `HOME/.config/sav/registry.toml`; a relative/empty `XDG_CONFIG_HOME` falls
  back to the absolute `HOME` path.

### Kvist binding (maerg) — delegated, not duplicated

`maerg/src/wizard.rs` `run_wizard_inner` creates the reusable profile by calling
`sav::run_registry_setup` (delegated, not re-implemented) and then writes only a
shared **binding** to `kvist.toml` (project) or
`~/.config/kvist/config.toml` (global): `[agent.registry] path = ...` and
`[agent.roles.ROLE] profile = NAME`. `persist_shared_binding` refuses to mix a
shared binding with local provider/profile definitions, and the completion
message states "no provider facts were copied." `AgentConfig::resolve_role_profile`
/ `resolve_named_profile` (`maerg/src/config.rs`) resolve the role's profile from
the shared registry and expose the pinned typed backend. The
`.kvist/providers.toml` project registry path is `sav::registry::RegistryPaths::for_project`,
i.e., sav's own registry convention, not a Kvist workflow concept.

## Generic-profile contract classification (`sav/CONTRACT.md`)

Statuses use the runbook terms. `R`/`C`/`D`/`I` denote requirement/contract/
design/implementation locators; evidence is the named test above.

### G-1 — sav owns reusable profile/setup mechanisms, not host
roles/task state/policy. Compliant.

`R` `AR-REQ-SETUP`/`AR-REQ-PROFILES`; `C` §`Boundary and ownership`.
Observed: no Kvist role names, task, policy, or grant concept appears in
`sav/src` (bounded search returned none); `ModelProfile` is documented as
"independent of application-specific roles" and `registry::Profile` as
"independent of role/tool authority." The capability lists `advertised` /
`tested` / `policy_enabled` are generic capability tiers, not roles.

### G-2 — `sav setup [--registry PATH] [--force]` performs shared profile
qualification and update. Compliant.

`C` §`Provided interfaces`. Observed: `sav setup` binds stdin/stdout to
`run_registry_setup`, uses the fixed qualification prompt, refuses to persist on
failed qualification, and honors `--force`. Evidence:
`setup_uses_fixed_prompt_and_refuses_failed_qualification`,
`setup_force_persists_profile_after_failed_qualification`.

### G-3 — `sav registry` is filesystem-only JSON inspection; listing includes
disabled profiles, selecting a disabled profile fails. Compliant.

`C` §`Provided interfaces`; `I` "Shared registry, setup, and pinned resolution."
Observed: `Command::Registry` loads `RegistryPaths` and emits resolved profiles
with no discovery/qualification; `resolve` refuses disabled profiles while
`profiles()` lists them. Evidence: `disabled_profiles_and_invalid_present_sources_fail_closed`.

### G-4 — `sav run` requires exactly one command template or stored profile and
`--allow-host-execution`; native profiles are refused in favor of `sav model`.
Compliant.

`C` §`Provided interfaces`. Observed: `run` returns
`HostExecutionNotAcknowledged` without the flag, requires exactly one of
`--command`/`--profile`, and maps a registry `Native` profile to an error
pointing at `sav model --profile`. Evidence: `host_execution_requires_explicit_acknowledgement`,
`setup_persists_a_profile_and_run_can_select_it`.

### G-5 — `sav model` exposes no tools and performs no host process execution.
Compliant.

`C` §`Provided interfaces`. Observed: `model` builds a `DirectModelTransport`
(native) with `tools: []` / `ToolChoice::None` and no process spawn; a
profile-selected transport is `transport_bounded`.

### G-6 — Shared registry typed schema: `Backend` native/external-agent,
capability lists, bounds, filesystem-only resolution, `command_spec` substitution,
no inline secrets. Compliant.

`C` §`Shared registry configuration (ADR 0015)`; `R` "Shared registry
configuration acceptance (ADR 0015)". Observed: tagged `Backend`, 64 KiB /
128-profile / 32-capability bounds, whole-definition precedence
(explicit>project>user), pinned source+digest resolution that never probes,
`command_spec` one-time `{prompt}`/`{model}`/`{cwd}`/`{reasoning_effort}`
substitution with `{context_files}` expansion, inline-credential refusal, and
external agents cannot produce a native transport. Evidence:
`registry_resolution_is_ordered_pinned_and_does_not_probe`,
`external_identity_is_structured_and_resolution_never_executes`,
`structured_argv_never_reinterprets_prompt_placeholders_or_shell_syntax`,
`rejects_inline_credentials_and_independent_registry_bounds`,
`explicit_source_is_required_and_whole_profiles_override_not_fields`.

### G-7 — Typed external-agent profiles always use zero process retries; a
positive `--max-retries` is rejected before spawning. Compliant.

`C` §`Provided interfaces`. Observed: `run` forces external `max_retries = 0`
and rejects an explicit positive count before spawn. Evidence:
`external_profiles_default_to_one_attempt_even_after_retryable_failure`,
`external_profiles_reject_positive_retries_before_spawning`,
`external_profiles_accept_explicit_zero_retries` (`tests/registry_cli_policy.rs`).

### G-8 — Raw command-template library helpers do not resolve backend identity
and are not a compatibility parser for registry files. Compliant (as to the
helpers).

`C` §`Data and schemas`. Observed: the *helpers* `render_command` /
`split_raw_command` (`src/command.rs`) are pure argv helpers; they do not parse
registry TOML or select a backend, and the `sav` CLI never reads a
`ModelProfile` command store — so this clause is compliant as written.
See **M-1** for the related retained finding that the public API additionally
retains a full second *store+wizard* (`ModelProfile` + `run_setup_wizard*` /
`collect_profile*` + `upsert_profile` / `load_profiles`), i.e., more than "raw
helpers," which the clause does not contemplate.

### G-9 — Bounded, provider-neutral setup: terminal-neutral I/O, maintained
provider templates, bounded advisory probes, explicit executable/model selection,
bounded discovery, fixed-prompt qualification, `--force` without interactive
bypass. Compliant.

`R` `AR-REQ-SETUP`. Observed: injected `BufRead`/`Write` streams; provider-owned
templates; 10 s probe with 64 KiB bound; numbered catalog choices with a final
custom choice; discovery bounded by time/bytes/count/id-size with no inference
prompt; fixed qualification prompt; failed qualification blocks persistence and
`--force` is non-interactive. Evidence: `unusable_provider_fallback_is_rejected_before_persistence`,
`cancelling_conventional_probe_does_not_request_a_fallback`,
`llama_server_lists_models_and_keeps_profile_name_separate`,
`llama_server_keeps_explicit_default_when_discovery_is_unavailable`.

### G-10 — Strict bounded TOML profiles; preserve unrelated formatting/values;
validate before and after; safe locations; synchronized atomic replacement.
Compliant.

`R` `AR-REQ-PROFILES`. Observed: both stores share `persist_configuration`
(no-clobber create, `sync_all`, parent sync, symlink refusal, 64 KiB) and
`is_valid_profile_name`; reads are bounded, `O_NOFOLLOW`/`O_NONBLOCK`. Evidence:
`update_preserves_comments_unrelated_values_and_profiles`,
`registry_upsert_roundtrips_and_refuses_symlinks`,
`invalid_existing_configuration_remains_unchanged`,
`rejects_invalid_profile_names_before_writing`.

## Kvist-binding contract classification (`maerg/CONTRACT.md`)

### K-1 — `[agent.registry] path` selects a shared Sav registry. Compliant.

`maerg` §`Shared registry configuration (ADR 0015)`. Observed:
`persist_shared_binding` writes `[agent.registry] path`; `RegistryPaths` resolves
it.

### K-2 — `[agent.roles.ROLE] profile = "NAME"` binds a shared named profile;
role policy remains local; no provider/model facts are copied by setup.
Compliant.

`maerg` §`Shared registry configuration (ADR 0015)`. Observed: the wizard writes
only the binding; the completion message states "no provider facts were copied";
`persist_shared_binding` refuses mixing a binding with local provider/profile
definitions; `AgentConfig::sync_roles_and_profiles` copies only command
templates resolved from the shared profile, not raw provider facts.

### K-3 — `resolve_role_profile`/`resolve_named_profile` expose pinned typed
backend data to the execution host. Compliant.

`maerg` §`Shared registry configuration (ADR 0015)`. Observed: both resolve from
`AgentConfig::shared_profiles` (the loaded shared registry) and return
`sav::registry::ResolvedProfile`; each shared role retains a `resolved_profile`
snapshot in the serialized approval input.

### K-4 — External-agent refusal for protected tasks remains mandatory; parsing
or qualification never grants process authority. Compliant.

`maerg` §`Shared registry configuration (ADR 0015)`; `C` §`Security and
authority`. Observed: `sav` parsing/qualification are filesystem-only or
bounded-capture and confer no execution authority; the Kvist host itself
refuses external agents for protected authoring
(`maerg/src/agent.rs:159`: "protected task authoring requires a native model
profile; external agents do not implement its broker contract"), and `sav run`
requires `--allow-host-execution`. (Separately, `skott/src/external.rs` supports
a durable-log external-agent path but rejects native-only
turn/context/tool-profile/token-limit overrides — a supported-agent
restriction, not the protected-task refusal named here.)

### K-5 — `kvist agent setup` remains a compatibility surface without duplicated
setup logic. Compliant.

`sa-06-implement-code` expected outcome. Observed: `maerg/src/wizard.rs`
`run_wizard_inner` delegates profile creation to `sav::run_registry_setup` and
adds only the save-location and role-binding steps; it does not re-implement
discovery, probes, or qualification.

## Discrepancies and observations (retained for human arbitration)

### M-1 (HIGH, mismatched) — an exported, tested, but unwired second
command-template setup/store coexists with the canonical registry.

- *Locator:* `sav/src/lib.rs:52,56-58` (re-exports `load_profiles`,
  `upsert_profile`, `collect_profile*`, `run_setup_wizard*`, `verify_profile`,
  `default_profile_config_path`); `sav/src/setup.rs:276-419`;
  `sav/src/profile.rs` (`ModelProfile`/`default_profile_config_path`/
  `load_profiles`/`upsert_profile`); `sav/CONTRACT.md` §`Provided interfaces`
  and §`Data and schemas`; `sav/IMPL.md` "Shared registry, setup, and pinned
  resolution" ("Separate exported `ModelProfile`, `load_profiles`,
  `upsert_profile`, and `run_setup_wizard*` APIs still operate a command-template
  TOML array-of-tables store … These are distinct observed library interfaces,
  not interchangeable shared-registry schemas").
- *Intended:* `sav setup` is "the" reusable setup; the command-template surface
  is a set of "raw command-template library helpers" that do not resolve backend
  identity and are not a registry parser.
- *Observed:* there is a second, complete profile *store + wizard + verification*
  (`ModelProfile` + `collect_profile*`/`run_setup_wizard*`/`verify_profile` +
  `upsert_profile`/`load_profiles`/`default_profile_config_path`) that is `pub`
  and unit-tested (`tests/setup.rs`, `tests/profiles.rs`), yet is wired to no CLI
  subcommand (`sav setup` uses the registry) and is read by neither `sav run`/
  `sav model` (`retired_profile_store_is_not_read_when_registry_is_absent`) nor
  by maerg or skott (zero references). It uses a disjoint placeholder vocabulary
  (`{prompt_json}`, `{target_directory}`, `{context_files}`) and a different
  provider default (Ollama) than the registry's `command_spec`
  (`{model}`, `{cwd}`), and it materializes HTTP providers as command strings
  where the registry uses typed native transports.
- *Why it matters:* a consumer who reads the public `sav` API can reach a
  "setup" that persists to a different file, with different placeholders and
  different provider encoding, than the one `sav setup` and Kvist use. This is
  the concrete source of the implementer's inability to certify that "model
  setup" is reusable: the reusable boundary is real, but the public surface
  advertises two different setup notions. Note this is **not** Kvist-workflow
  leakage (see N-5/N-6) — it is an internal sav duplication that obscures the
  canonical boundary.
- *Arbitration options:* (a) deprecate or remove the legacy
  `ModelProfile`/`run_setup_wizard*`/`verify_profile`/`upsert_profile`/
  `load_profiles`/`default_profile_config_path` surface, retaining only
  `render_command`/`split_raw_command` as the contract's "raw command-template
  helpers"; (b) explicitly document it in `CONTRACT.md` as a retained-but-
  superseded library surface and name the two stores, their files, and their
  placeholder vocabularies; or (c) record a reasoned approved deferral with the
  retirement condition.

### N-1 (LOW, observation) — the two stores use disjoint placeholder
vocabularies and are not interchangeable.

- *Locator:* `sav/src/command.rs` (`{prompt}`, `{prompt_json}`, `{context_files}`,
  `{target_directory}`, `{reasoning_effort}`) vs. `sav/src/registry.rs`
  `ExternalAgent::command_spec` (`{prompt}`, `{model}`, `{cwd}`,
  `{reasoning_effort}`, `{context_files}`).
- *Observed:* a `ModelProfile.command` rendered by `render_command` is not the
  same shape as a registry `ExternalAgent` argv; HTTP providers are command
  strings in one store and typed native transports in the other.
- *Effect:* bounded to the library (no host executes the legacy store), but the
  divergence is not named in the contract. Retain alongside M-1.

### N-2 (LOW, observation) — provider-template and menu divergence between the
two surfaces.

- *Locator:* `sav/src/setup.rs` `run_registry_setup` menu (`6) Custom
  executable`, Claude not offered) vs. `collect_profile_with_options` menu
  (`6) Custom Wrapper Script`, Ollama default, `--context '{context_files}'`
  custom template).
- *Observed:* the maintained-template requirement is satisfied on the canonical
  surface; the legacy surface's templates are redundant. Retain with M-1.

### N-3 (LOW, interpretation) — ACP host-discovery acknowledgement asymmetry.

- *Locator:* `sav/src/catalog.rs` `discover_acp` (requires
  `options.allow_host_discovery`); `sav/src/main.rs` `models`
  (`--allow-host-discovery`); `sav/src/setup.rs:122,537,570` (hard-codes
  `allow_host_discovery: true`).
- *Observed:* `sav models` requires the explicit flag; `sav setup` treats the
  setup action itself as the acknowledgement (matching `AR-REQ-SETUP`). The
  asymmetry is not explicitly stated in the contract, which only names
  `--allow-host-discovery` for `models`. Retain as a documentation gap.

### N-4 (LOW, evidence limit) — `tests/setup.rs` exercises the superseded
surface, not the canonical one.

- *Locator:* `sav/tests/setup.rs` (imports `collect_profile*`, `verify_profile`);
  canonical coverage lives in `sav/tests/cli.rs` (`setup_*`) and
  `sav/tests/registry.rs`.
- *Observed:* the file named "setup" tests the legacy wizard/store; the canonical
  `run_registry_setup`/`sav setup` path is covered under "cli"/"registry." This
  is an evidence-attribution observation that reinforces M-1, not a defect.

### N-5 (LOW, compliant) — Kvist binding does not duplicate setup logic.

- *Locator:* `maerg/src/wizard.rs` `run_wizard_inner`,
  `persist_shared_binding`; `maerg/src/config.rs` `resolve_role_profile`/
  `resolve_named_profile`; `maerg/CONTRACT.md` §`Shared registry configuration
  (ADR 0015)`.
- *Observed:* profile creation is delegated to `sav::run_registry_setup`; only a
  shared binding is written ("no provider facts were copied"); roles resolve the
  pinned typed backend; and the Kvist host refuses external agents for protected
  authoring (`maerg/src/agent.rs:159`). This satisfies K-1…K-5 and confirms the
  delegation boundary.

### N-6 (LOW, compliant) — no Kvist role/workflow semantics are present in sav.

- *Locator:* `sav/src` (bounded search for `kvist.toml`, `agent.roles`, role
  names, task/policy concepts — none found); `sav/src/registry.rs`
  `RegistryPaths::for_project` (`.kvist/providers.toml`).
- *Observed:* sav contains no role names, task, policy, or grant concept.
  `.kvist/providers.toml` is sav's own neutral project-registry path
  (part of documented registry resolution), not a Kvist workflow reference. This
  directly answers the implementer's concern: the Kvist-workflow boundary is
  preserved; the residual risk is the internal sav duplication in M-1, not a
  cross-component leak.

## Discrepancy summary

| ID  | Clause / finding                                                        | Status       | Severity |
|-----|-------------------------------------------------------------------------|--------------|----------|
| G-1 | sav owns reusable setup, not host roles/policy                          | compliant    | none     |
| G-2 | `sav setup [--registry] [--force]` qualification/update                 | compliant    | none     |
| G-3 | `sav registry` filesystem-only inspection; disabled fails               | compliant    | none     |
| G-4 | `sav run` single template/profile + host acknowledgement; native refused | compliant    | none     |
| G-5 | `sav model` no tools, no host process execution                         | compliant    | none     |
| G-6 | Shared registry typed schema, resolution, no inline secrets             | compliant    | none     |
| G-7 | External-agent zero retries; positive rejected before spawn             | compliant    | none     |
| G-8 | Command-template helpers do not resolve backend / parse registry        | compliant (M-1) | high   |
| G-9 | Bounded provider-neutral setup; fixed-prompt; `--force` non-interactive | compliant    | none     |
| G-10| Strict bounded TOML; atomic; formatting preserved; safe locations       | compliant    | none     |
| K-1 | `[agent.registry] path` selects shared Sav registry                     | compliant    | none     |
| K-2 | `[agent.roles.ROLE] profile` binds; no provider facts copied            | compliant    | none     |
| K-3 | `resolve_role_profile`/`resolve_named_profile` pinned typed backend     | compliant    | none     |
| K-4 | External-agent refusal for protected tasks; no authority from parsing   | compliant    | none     |
| K-5 | `kvist agent setup` delegates, no duplicated setup logic                | compliant    | none     |
| M-1 | Exported, tested, unwired second command-template setup/store           | mismatched   | high     |
| N-1 | Disjoint placeholder vocabularies; stores not interchangeable           | observation  | low      |
| N-2 | Provider-template / menu divergence between surfaces                    | observation  | low      |
| N-3 | ACP host-discovery acknowledgement asymmetry (models vs setup)          | observation  | low      |
| N-4 | `tests/setup.rs` covers the superseded surface                         | evidence limit | low     |
| N-5 | Kvist binding delegates setup; no duplication                           | compliant    | low      |
| N-6 | No Kvist role/workflow semantics in sav                                 | compliant    | low      |

**Total hard mismatches:** 1 (M-1, high)
**Observations retained for arbitration:** 6 (all LOW), of which N-5/N-6 are
compliant confirmations of the boundary and N-1/N-2/N-3/N-4 are documentation/
attribution limits tied to M-1.

## Conclusion

Every setup and integration requirement was classified. The generic-profile
contract (`sav/CONTRACT.md`) and the Kvist-binding contract
(`maerg/CONTRACT.md`) are **compliant on all fifteen clause comparisons**: sav
owns reusable, provider-neutral setup and profile storage; `sav setup` is the
canonical, bounded, fixed-prompt, `--force`-without-interactive-bypass path;
the shared registry is typed, filesystem-only, and credential-safe; and Kvist
binds those profiles by delegation without duplicating setup logic or copying
provider facts. This confirms the extracted configuration boundary: **no Kvist
workflow semantics leaked into sav** (N-6), and the delegation boundary holds
(N-5, K-1…K-5).

The one retained mismatch (M-1, high) is that the public `sav` API still
advertises a second, complete command-template setup/store (`ModelProfile` +
`run_setup_wizard*`/`collect_profile*`/`verify_profile` + `upsert_profile`/
`load_profiles`/`default_profile_config_path`) that is exported, unit-tested,
but wired to no CLI subcommand and consumed by no host. That internal duplication
—not a cross-component leak—is the concrete reason the implementer could not
certify reuse, and it is recorded here for explicit arbitration (retire it, or
document it as a retained-but-superseded surface). No clause was classified
approved-deferred in its own right. This record is the independent gate
disposition for the `sa-06` model-setup chain; it is not self-certified,
establishes no clause-level certification beyond the record above, and does not
constitute acceptance or sign-off. Formal sign-off is blocked while M-1 remains
unarbitrated; acceptance, if required, remains subject to the separate review
receipt/acknowledgement or a reasoned exception per the change-and-compliance
rule.
