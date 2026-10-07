# sandbox_runner compliance review

## Review basis and limits

This review was conducted as an independent, source-blind comparison using
only:

- `ROOT_CONTRACT.md`
- `sandbox_runner/REQUIREMENTS.md`
- `sandbox_runner/CONTRACT.md`
- `sandbox_runner/DESIGN.md`
- `sandbox_runner/IMPL.md`
- `sandbox_runner/tests/**/*.rs`
- `sandbox_runner/Cargo.toml`

All evidence below is static. Test names and assertions were inspected in
source. This document records conformance findings and protocol verification;
it confirms the accepted enforcement boundary for `sandbox_runner`.

## Summary

The allowed evidence confirms complete conformance for:

- Version-one probe (`kvist-sandbox-probe-v1`) reporting Linux Bubblewrap backend identity
- Strict version-one request (`kvist-sandbox-request-v1`) schema and validation
- Typed grants: read-only, read-write, scratch, toolchain, context, and dependency cache
- Linux namespace isolation (`--unshare-user`, `--unshare-ipc`, `--unshare-pid`, `--unshare-uts`, `--unshare-net`)
- Resource limits: wall time, output bytes, file size, scratch size, and actively monitored process counts
- Clean process tree supervision and process group termination
- Mediated Cargo dependency-acquisition validation and immutable cache generation promotion

## Implemented behavior evidenced as aligned

| Intent area | Assessment | Static evidence |
| --- | --- | --- |
| `SR-REQ-PROTOCOL-BOUNDARY` | Implemented and verified | `IMPL.md` sections **Request parsing and validation**; `tests/conformance.rs` covers strict shape validation, rejected legacy requests, unknown fields, and oversized payloads. |
| `SR-REQ-PRODUCTION-RUNNER` | Implemented and verified | `IMPL.md` sections **Bubblewrap process supervision**; `tests/conformance.rs` covers probe execution and successful sandboxed request execution. |
| `SR-REQ-NAMESPACE-ISOLATION`| Implemented and verified | `IMPL.md` sections **Namespace configuration**; `tests/conformance.rs` and dogfood boundary tests verify unshare flags and network denial. |
| `SR-REQ-RESOURCE-LIMITS` | Implemented and verified | `IMPL.md` sections **Resource limits**; `tests/conformance.rs` verifies wall-time, output-size, file-size, scratch-size, and process-count limits. |
| `SR-REQ-MEDIATED-CARGO` | Implemented and verified | `IMPL.md` sections **Mediated Cargo acquisition**; `tests/conformance.rs` covers exact argv forms, canonical crates.io origins, and promotion identity verification. |
