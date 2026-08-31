# Standards and Interoperability Profile

## Status and interpretation

This document records which external standards and practices Kvist follows,
uses as inspiration, supports through native artifacts, or has considered and
deferred. It prevents informal references to "standards compliance" from
becoming broader claims than the project can demonstrate.

`Aligned` means Kvist deliberately follows the applicable concepts.
`Conformant` is used only when the relevant conformance requirements and
evidence have been assessed. `Interoperable` means Kvist can import, export, or
reference the named format without claiming conformance in unrelated areas.

## Adopted and aligned practices

| Standard or practice | Kvist position | Application |
| --- | --- | --- |
| ISO/IEC/IEEE 42010:2022 | Conceptually aligned; formal conformance not yet claimed | `ARCHITECTURE.md` identifies the system, stakeholders, concerns, architecture drivers, component and interaction views, correspondences, decisions, and rationale. The standard deliberately does not prescribe a file format or method. |
| arc42 | Tailored content profile | Context, constraints, building blocks, runtime interactions, cross-cutting concepts, decisions, quality concerns, risks, and glossary concepts inform architecture and design templates. Unneeded sections are omitted. |
| C4 model | Selective visualization convention | Context, container, component, and dynamic views may be used when valuable. Kvist's directory-to-component mapping is a Kvist convention, not a C4 requirement. |
| RFC 2119 and RFC 8174 (BCP 14) | Adopted for normative prose | Uppercase MUST, SHOULD, MAY, and related terms have BCP 14 meanings only where the document includes the interpretation statement. Ordinary lowercase prose is not treated as a requirement level. |
| Architecture Decision Records | Adopted practice | Significant decisions use numbered Markdown files containing status, context, decision, alternatives, and consequences. Superseding decisions create a new ADR rather than rewriting history. |
| Semantic Versioning 2.0.0 | Intended for released public contracts | A public API or artifact format must be explicitly declared before SemVer compatibility claims are meaningful. Pre-release internal schemas remain independently versioned. |

ISO/IEC/IEEE 15288 and 12207 informed the recursive lifecycle, role separation,
configuration management, traceability, verification, and review gates. Kvist
does not claim process conformance to either lifecycle standard because doing
so would require a broader organizational assessment with no current product
benefit.

IEEE 1016 software design description guidance was considered. Its enduring
view-based separation of design information is useful, but Kvist uses
ISO/IEC/IEEE 42010 plus arc42 as the primary architecture/design profile rather
than claiming IEEE 1016 conformance or reproducing its document organization.

## Interface-native standards

`CONTRACT.md` is authoritative for behavioral semantics. When a boundary has a
useful machine-readable syntax, the contract should reference the native
definition and state its exact dialect/version and authority:

| Boundary | Preferred machine-readable definition | Current support |
| --- | --- | --- |
| HTTP API | OpenAPI | May be referenced from `CONTRACT.md`; validation/import/export deferred until an HTTP boundary needs it |
| Event or message API | AsyncAPI | May be referenced; validation/import/export deferred |
| JSON or YAML data | JSON Schema 2020-12 or an explicitly named later dialect | Recommended for machine-consumed artifact payloads; Kvist's Rust parsers remain authoritative for current internal schemas |
| RPC | Protocol Buffers/gRPC or the selected protocol's IDL | May be referenced when selected by architecture |
| WebAssembly component boundary | WIT | May be referenced when a WASI component boundary exists |
| Rust library | Public Rust types, rustdoc, compile-time checks, and contract tests | Native current practice |
| CLI | Parser definition, deterministic help/reference output, and integration tests | Native current practice; no universal CLI schema is claimed |

Machine-readable definitions describe syntax and data shape well, but usually
do not capture all authorization, ordering, retry, partial-effect,
compatibility, or operational semantics. Those remain in `CONTRACT.md`.

## Information retained for future interoperability

Kvist records only information needed by its workflow or reasonably required
for a later loss-aware adapter:

- stable component, contract, interface, requirement, and decision IDs;
- artifact or schema kind and exact format/dialect version;
- provider and consumer direction;
- component path and provider-owned definition path;
- responsibility and explicit non-responsibility;
- operations, messages, inputs, outputs, encodings, and validation rules;
- observable state effects and behavioral guarantees;
- error, timeout, cancellation, retry, ordering, concurrency, and idempotency
  semantics where applicable;
- trust, authorization, confidentiality, integrity, and resource boundaries;
- compatibility, deprecation, and migration policy;
- requirement, decision, test, and evidence links;
- exact reviewed revisions, producer version when exported, integrity digest,
  validation result, approval state, and provenance.

Kvist does not collect enterprise ownership taxonomies, procurement metadata,
cost models, organizational charts, deployment inventories, or generalized
modeling data unless a concrete workflow needs them.

## Considered and deferred interchange

**Requirements Interchange Format (ReqIF):** considered as a future
requirements import/export target. Full ReqIF authoring would add substantial
complexity and weak value for filesystem-native Markdown. Stable requirement
IDs, text, rationale, status, links, and verification references are retained
so a loss-aware adapter remains possible.

**ArchiMate exchange and UML/SysML/XMI:** considered for architecture-model
exchange but deferred. They are broader than Kvist's component workflow and
round-tripping Markdown, diagrams, contracts, and approval provenance would be
lossy. A future adapter may export selected views without making the external
model canonical.

**Structurizr DSL:** a practical optional C4 export target, not a formal
architecture interchange standard. Export can be added when diagrams need
automation. Imports must be reviewed because a C4 model cannot carry all Kvist
requirements, contract semantics, task state, and compliance evidence.

## Import and export rules

An adapter must declare which information it preserves, transforms, or cannot
represent. Import validates before activation, treats all external content as
untrusted, rejects unknown required semantics, retains source provenance, and
never silently turns an informative field into a normative Kvist requirement.
Export declares the schema and producer versions, uses deterministic encoding
where the target permits it, and must not imply broader standards conformance
than has been assessed.

Complex adapters remain deferred until a real integration requires them.
Preserving the identifiers, relationships, versions, and provenance above is
the current compatibility commitment.
