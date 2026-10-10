# Galla sandbox toolchain testing review

## Conclusion

Full end-to-end coverage was **not already present**. It is possible to test
real tool discovery, execution and complete **installed** rustup inventory
visibility without modifying the host installation. New strict native tests
now demonstrate this for explicitly constructed Galla requests.

The initial testing change did not broaden consumer mount authority.
Following explicit human approval, **Skott now automatically exposes every
validated standard-layout installed Rust toolchain and target read-only**.
Its project pin or host default remains the initial selection; the agent can
choose another installed version. Maerg's closed Cargo verification path
remains intentionally single-toolchain. Maerg Rust authoring now exposes the
installed inventory using separate read-only System/Toolchain resources.
Both authoring paths initialize invocation-local writable selection settings;
`rustup default stable` uses the real installed rustup offline, never host settings.
With explicit human approval in ADR-0014, maerg authoring also selects declared
Cargo workspace/path-dependency build context read-only. Its ordinary workspace
trial exercises inheritance, formatting when installed, edited source and newly
authored test discovery, nested provider intent/state exclusion and denied
provider mutation. Writable peer aliases are rejected. This is build context,
not implicit prompt propagation; closed verification is unchanged.

The final repair was independently source-observed and its ordinary native
tests re-executed: Skott inventory **1 passed**, maerg authoring **3 passed**,
with four older verification trials ignored in that ordinary invocation.
A subsequent explicit native chain ran **7 maerg and 11 Skott tests**, all
passing with none ignored. Affected library tests reported **670 passed**
(one unrelated ignored test); the complete Sav suite reported **147 passed**.
Strict affected all-target Clippy and formatting checks passed. No remote CI
run is claimed.

A separate source-blind comparison found no new scoped Rust/build-context
mismatch. It retained the pre-existing **MAERG-WRITE-SCOPE** discrepancy:
broader component-root writing is deferred despite broader contract wording;
this repair permits only existing source/test-root writes and does not make
Cargo metadata writable. Case-level coverage limits and the final observer's
uninspected Skott inventory-helper internals remain explicit. This is not
acceptance or blanket compliance certification.

Skott's separate native `rustup_inventory` test drives its real registry and
executor, not manually authored Galla grants. It compares names, compiler/
Cargo/rustdoc versions and target/component listings; compiles all installed
targets; runs native programs and Cargo unit/doctests with `cargo +name`,
invocation-local `RUSTUP_TOOLCHAIN` and `rustup run`; and checks the compiler/
rustdoc actually used by build scripts. It also verifies read-only uninstall/
target-removal failures and private settings changes, rejects unavailable/option-like selectors,
and compares before/after host rustup content and metadata. It passed locally
for the same eight toolchains and 14 installed-target combinations.

All host installations stay read-only. Preparation only reads them and creates
owned private temporary staging; compiler/cache/target state stays in sandbox
scratch. It never installs, updates, repairs or removes a host toolchain.
Filesystem reads may still update access times. New target/version provisioning
or detected drift requires a restart; unsafe/custom-linked installations fail
explicitly, rather than yielding an incomplete advertised inventory.
The scoped independent authority audit reported no high-confidence finding;
the separate source-blind comparator reported no blocking mismatch for the
demonstrated inventory/selection behavior. Final defensive membership/option
guards have audit/regression evidence rather than a refreshed clean-slate
observation. This is not component acceptance or unconditional certification.

## Existing coverage and findings

| Coverage | What it establishes | Gap or limitation |
| --- | --- | --- |
| `galla/tests/conformance.rs` | Protocol validation, Cargo grant/environment rules, native probe and a successful simple request | No comprehensive tool discovery, multi-version inventory comparison or real language builds. Its introductory comment still incorrectly says executable enforcement is outside the file. |
| `skott/tests/rust_build_environment.rs` and `rustup_inventory.rs` | Selected-toolchain full build chain plus automatic full-inventory/default discovery, every installed target and alternate-version compiler/Cargo/doc execution | Inventory/default coverage is non-ignored and fails on missing prerequisites. Other native cases remain opt-in. |
| `skott/tests/toolchain_discoverability.rs` | Ignored PATH/version probes for Python, Node, Go, C and combined profiles | Version output is not execution evidence. Node's “contains a dot” assertion is weak; GCC's literal `(GCC)` assertion is distribution-sensitive. The combined case has no `rustup` inventory comparison. These tests are not explicitly enabled in the current native CI chain. |
| `maerg/tests/sandbox_toolchain.rs` | Ordinary Rust authoring inventory/default parity, target compilation, offline builds and host snapshots; non-Rust environment preservation; explicit native verification grants | Authoring/non-Rust regressions are non-ignored. Verification trials remain opt-in and distinct from the closed Cargo topology. |
| `maerg/tests/language_offline_e2e.rs` | Language-aware host provisioning and real network-denied vendored verification | Missing runner/backend/toolchains or unavailable provisioning can return early and appear as passing tests. Provisioning may use the host network; these are not read-only host-inventory tests. They do not prove all toolchains are available in every consumer profile. |
| `docs/sandbox-toolchains-and-vendoring.md` | Describes the different consumer mount topologies | Its prior blanket assurance that discoverability tests establish all tooling is functional was too strong. PATH visibility, compiler operation, dependency provisioning and consumer profile selection need separate evidence. |

Existing independent Galla security/compliance re-verification remains
blocked/pending in `galla/TODOS.yaml`; completed placeholder-era receipts are
not reliable evidence. This testing review is not a security audit, an
independent `IMPL.md` derivation, or compliance certification.
The new test-specific queue likewise records independent audit and compliance
review as pending; updated intent is not represented as newly accepted.

## Implemented native tests

New file: [`../galla/tests/toolchain_e2e.rs`](../galla/tests/toolchain_e2e.rs).
Both tests invoke the actual independently built `galla-runner`, transmit a
validated version-one JSON request, and execute through real Bubblewrap with
the network denied. They are ignored in the portable/default test invocation
but **explicitly executed in the Linux quality CI job**. Once invoked,
missing prerequisites and any execution/query failure are failures, not skips.

### System toolchains

`system_toolchains_are_found_and_execute_real_programs`:

- Finds `python3`, `node`, `go`, `gcc` and `g++` through sandbox PATH.
- Runs Python assertions and a JavaScript assertion/program.
- Compiles and executes a Go program, with local-only toolchain selection,
  dependency proxy and checksum service disabled and private caches.
- Compiles and executes C and C++ programs, exercising compiler, assembler,
  linker, system headers and runtime libraries.
- Requires exact execution-output markers, not loose version substrings.

These dependency-free fixtures do not download anything or provision host
tools. They test the system binaries actually available in the mounted
system layout, not every installation discoverable through the host's PATH.
Home-managed Python environments, Node managers, Go toolcache installations,
package managers (`uv`, npm/pnpm, Conan/vcpkg), and third-party dependencies
still need their own approved mounts and provisioning tests.

### Full installed Rust inventory

`rustup_reports_and_uses_every_host_toolchain_without_host_mutation`:

- Reads the host `rustup toolchain list`, resolves every installed compiler
  root and registers every name in a private generated rustup home.
- Creates sandbox-native registration links, mounts each resolved installation
  and the generated home read-only, and does not mount the ambient host home,
  Cargo configuration/credentials or host rustup settings.
- Compares the complete name set, not just the selected installation.
- For **every** installation, compares `rustc -Vv`, Cargo and rustdoc versions,
  `rustup target list`, `rustup target list --installed` and
  `rustup component list --installed` against successful host queries.
- Compiles a library for **every installed target**, including cross targets.
  This checks that the target standard library is usable, not merely listed.
- Compiles and runs a native executable and runs a real dependency-free
  `cargo test --offline --locked` fixture for every installed toolchain.
- Attempts toolchain uninstall, installed-target removal and settings changes;
  requires explicit read-only-filesystem errors, not arbitrary nonzero exits.
- Requires at least **two toolchains** and at least **one non-host target**,
  preventing a trivial single-installation/single-target pass from masquerading
  as multi-version/cross-target coverage.

Toolchain listing annotations such as `(active, default)` are deliberately
excluded from the inventory-name comparison. The generated home's default
and selection are explicit sandbox settings, not copies of host defaults or
directory overrides. Target/component query lines and version output are
compared exactly apart from line ordering.

CI separately provisions Rust `1.95.0` alongside stable and adds
`wasm32-unknown-unknown` to stable **before** tests. System Python, Node, C/C++
and Go prerequisites are likewise installed by CI setup, not by test bodies.

## Host non-mutation evidence and exact limits

The Rust test streams SHA-256 over regular files and captures directory/file
paths, modes, sizes, modification/change timestamps and symlink destinations
before and after execution. It covers the host rustup tree and resolved
external linked toolchain roots, detecting addition, deletion, replacement,
content changes and tracked metadata changes. Final comparison runs even
when sandbox execution assertions fail. There is no restoration that could
hide a mutation.

Reads may update filesystem **access times**; these are deliberately excluded.
Therefore “no alteration of any filesystem metadata whatsoever” is not a
claim these tests can honestly make. The actual guarantee tested is no change
to installed toolchain content, registrations, settings or tracked metadata.
Only newly owned temporary fixture directories are written by the tests;
their cleanup does not remove or rewrite any installed toolchain.

Other limits:

- “Available” means locally installed toolchains and the target catalogue
  reported by their existing rustup manifests. It does not mean every remote
  release or target downloadable from Rust servers. Network-denied tests
  cannot validate remote freshness or install missing versions/targets.
- Cross targets are compiled as libraries, not linked into or executed as
  foreign-platform executables. Linkers, SDKs, emulators and native testing are
  separate prerequisites; this does not expand Kvist's Linux-only support.
- Custom linked installations are resolved and included in snapshots. Rustup
  installations without target/component manifests cannot satisfy the rustup
  query assertions; this fails explicitly rather than omitting them. A
  separate custom-toolchain test could compare `rustc --print target-list`
  and compile explicitly known installed targets, but that would not prove
  rustup can enumerate metadata it does not possess.
- The fixture uses Cargo lock format 3 and Rust edition 2021. Very old or
  incomplete installations unable to build it fail explicitly. It is not a
  promise of support for arbitrary historic Rust releases.
- Inventory changes concurrently made by another host process are detected as
  failures, not attributed conclusively to the sandbox. Run without concurrent
  rustup installation/update activity.
- The protocol bounds grants and request size. An inventory exceeding those
  bounds fails validation rather than silently dropping installations.
- End-to-end execution evidence is not proof of complete sandbox isolation or
  protection against every possible malicious executable.

## Recorded local result

Both Galla native tests passed using the real runner and Bubblewrap. The Rust
trial covered these eight installed registrations:

`stable`, `nightly`, `1.64.0`, `1.67.1`, `1.85.0`, `1.94`, `1.94.0`,
and `1.95.0` (all with `x86_64-unknown-linux-gnu` installation suffixes).

There were **14 installed toolchain/target combinations** across six distinct
target triples, including i686 Linux, macOS, Windows GNU/MSVC and WebAssembly.
All installed-target library compilations, all eight native program runs and
all eight locked offline Cargo test runs succeeded. Host content/metadata
comparison found no changes. System Python, Node, Go, C and C++ program
execution also succeeded.

The existing Galla regression suite passed (26 library tests, 41 conformance
tests and two scaffold tests); Galla formatting and all-target Clippy checks
also passed. CI configuration was updated but no remote CI run is claimed.

## Reproduction

Prerequisites: Linux with working Bubblewrap, `/usr/bin/rustup`, at least two
installed toolchains with a non-host target, and system Python/Node/Go/C/C++.
Provision missing prerequisites separately on the host; tests never do so.

```bash
cargo build --locked -p galla --bin galla-runner
KVIST_GALLA=/opt/target/debug/galla-runner \
  cargo test --locked -p galla --test toolchain_e2e -- \
  --ignored --test-threads=1 --nocapture
```

Adjust the runner path to the configured target directory; it must be outside
the repository. No model/provider, dependency download or pre-existing vendor
tree is required. Passing this command establishes the explicit-grant
enforcement behavior described above, not consumer preparation. For actual
automatic Skott and maerg preparation, additionally run:

```bash
KVIST_RUST_TEST_RUNNER=/opt/target/debug/galla-runner \
  cargo test --locked -p skott --test rustup_inventory -- \
  --test-threads=1 --nocapture
KVIST_GALLA=/opt/target/debug/galla-runner \
  cargo test --offline --locked -p kvist --test sandbox_toolchain -- \
  --test-threads=1 --nocapture
```
