# Kvist

[![Rust](https://github.com/Fairglow/kvist/actions/workflows/rust.yml/badge.svg)](https://github.com/Fairglow/kvist/actions/workflows/rust.yml)
[![License: AGPL v3+](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)

Kvist is a filesystem-native, architecture-driven tool for human-directed AI
development. The project is still a work in progress; [Status](#status)
describes what is ready today. Its durable hierarchy is `VISION.md` ->
`ARCHITECTURE.md` -> per-component `REQUIREMENTS.md` + `CONTRACT.md` +
`DESIGN.md` -> `TODOS.yaml` -> `IMPL.md`. Its current interface is
command-line based; graphical and editor integrations are planned without
changing that model. Kvist currently builds and runs on Linux only. macOS and
Windows support is intentionally deferred until native maintainers and test
environments can validate their execution boundaries.
The product vision is defined in [`VISION.md`](VISION.md), the approved system
structure in [`ARCHITECTURE.md`](ARCHITECTURE.md), detailed strategy in
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md),
and the standards posture in [`docs/standards.md`](docs/standards.md).

## Status

Kvist is a work in progress. Several parts build and run today, and the
`kvist` CLI already enforces queue ordering and durable transitions, but the
project as a whole is not yet ready for production use.

The only component that is ready and released is **blad**, the terminal
Markdown viewer and streaming pager for skott session transcripts and
standalone files. It is published on crates.io as
[`kvist-blad`](https://crates.io/crates/kvist-blad). The remaining components
(`maerg`, `sav`, `galla`, and `skott`) are still under active development. See
[Intended lifecycle and current scope](#intended-lifecycle-and-current-scope)
for what the CLI does and does not yet automate.

## Why Kvist

AI coding tools are increasingly capable at producing code. The harder problem
is keeping product intent, architecture, authority, and evidence coherent as
those tools act. Important decisions can otherwise remain in chat history,
context can cross component boundaries without review, and successful
execution can be mistaken for correctness.

Kvist provides a durable control layer around AI-assisted development. It does
not try to replace coding agents or model frameworks. It gives them explicit,
version-controlled work to perform; limits the context and authority they
receive; and keeps intended behavior separate from independently observed
implementation evidence.

The human remains the architect and final arbiter. Agents may propose,
implement, document, and review, but an agent does not approve product intent
or certify its own work. When evidence and intent disagree, Kvist preserves the
discrepancy for a person to resolve rather than silently choosing a side.

This makes Kvist most relevant to teams that value architectural continuity,
inspectable state, bounded execution, and reviewable evidence more than
unrestricted autonomy. See [Why Kvist](docs/why-kvist.md) for a grounded
comparison with coding agents, specification kits, agent frameworks,
sandboxes, and governance tools.

The [security-first runner guide](docs/runner-hardening.md)
documents the implemented improvements, native-helper installation, headless
llama-server usage, authority limits and independent qualification record.

## Components

Kvist is one Cargo workspace of five peer components. The root manifest
(`Cargo.toml`) builds them all; each component owns a directory with the
five-artifact intent set (`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`,
`TODOS.yaml`, `IMPL.md`) and its own README. The approved decomposition is
the canonical component table in [`ARCHITECTURE.md`](ARCHITECTURE.md).

| Component | Directory | Binary         | Role                                                                                                         | Status                  |
| --------- | --------- | -------------- | ------------------------------------------------------------------------------------------------------------ | ----------------------- |
| **maerg** | `maerg/`  | `kvist`        | The CLI and workflow engine: project artifacts, discovery, status, queues, task lifecycle, policy, evidence. | In progress             |
| **sav**   | `sav/`    | `sav-run`      | Provider-neutral prompt acquisition, command rendering, process supervision, profiles, model transport.      | In progress             |
| **galla** | `galla/`  | `galla-runner` | Independent Linux enforcement boundary: request validation and Bubblewrap isolation.                         | In progress             |
| **skott** | `skott/`  | `skott`        | Interactive/headless sandboxed workspace agent shell.                                                        | In progress             |
| **blad**  | `blad/`   | `blad`         | Terminal Markdown viewer and streaming pager for skott session transcripts and standalone files.             | Released (`kvist-blad`) |

**Dependency direction.** `maerg` (the `kvist` CLI) depends on `sav` as a
library and speaks to `galla` only through the versioned sandbox protocol.
`skott` depends on `sav`, `galla`, and `blad` (its Markdown renderer). `blad`
has no runtime dependency on the other workspace components. `galla` is an
independent enforcement boundary: nothing imports its types, and it receives
no maerg authority. The maerg and the
installed runner communicate only through the versioned protocol, so the
enforcement boundary stays independently installed and separately approved.

**Standalone use.** `skott` and `blad` are standalone binaries that do not
require the `kvist` CLI. `skott` is a self-contained agent shell driven by its
own TOML configuration; `blad` opens Markdown files directly, with an opt-in
`sessions` Cargo feature for skott transcript commands. Kvist's all-feature
build recipes include that feature. Both can also be launched from Kvist
(`kvist prompt` runs `skott`; `blad` views the resulting transcripts). `sav` and `galla` are
reusable libraries with their own CLIs (`sav-run`, `galla-runner`) and are
usable independently of the `kvist` CLI.

Component specifics — what each is, its purpose, and how to build and run it —
live in the component READMEs:
[`maerg/README.md`](maerg/README.md),
[`sav/README.md`](sav/README.md),
[`galla/README.md`](galla/README.md),
[`skott/README.md`](skott/README.md), and
[`blad/README.md`](blad/README.md).

The `kvist` command-line reference — the full command contract, configuration
and platform policy, sandboxed task execution, the artifact and TODO-queue
formats, and the task execution boundary — is documented in the
[`maerg/README.md`](maerg/README.md), since `maerg` is the component that
builds the `kvist` binary.

## License and contributions

Kvist and the in-repository `sav` crate are copyright (C) 2026
Stefan Lindblad and licensed under the
[GNU Affero General Public License, version 3 or later](LICENSE).

The AGPL permits personal, open-source, and commercial use under its terms.
Organizations that need to distribute, embed, modify, or operate Kvist without
AGPL obligations may request separate commercial terms as described in
[`COMMERCIAL-LICENSE.md`](COMMERCIAL-LICENSE.md). No permission beyond the
AGPL is granted unless both parties execute a separate written agreement.

External code contributions are not accepted at this stage. Bug reports,
use-case feedback, and design discussion remain welcome. See
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Intended lifecycle and current scope

Kvist's direction remains structure before syntax. The human architect starts
with `VISION.md`, approves `ARCHITECTURE.md`, decomposes the system into
components, and approves each component's requirements, consumer contract, and
private design. A designer derives the traceable queue from those documents.
Tests precede implementation, followed by security audit.

A clean-slate documenter then derives `IMPL.md` from source and test evidence
without reading any intent artifact, queue, prior record, architecture/root
intent, prior review, chat history, or Git history. A different source-blind
reviewer compares the fresh record and test evidence with `REQUIREMENTS.md`,
`CONTRACT.md`, and `DESIGN.md`. The implementer cannot self-certify.
`IMPL.md` is not user documentation; public integration material belongs under
`docs/`.

The CLI currently enforces queue ordering and durable transitions, but it does
not yet automate architect/designer agents, the interview, clean-slate
documentation, source-blind comparison, arbitration, editor integration,
daemon, LSP, or web UI flows. Those are planned capabilities, not current
commands. See
[`GUIDE.md`](GUIDE.md) for the accurate manual workflow and
[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
for the enduring architecture and phased direction.
