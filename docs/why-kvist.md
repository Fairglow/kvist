# Why Kvist

Kvist is a control and evidence layer for human-directed AI development. It is
intended for developers and architects who want to use capable coding agents
without making an agent conversation, provider-specific runtime, or successful
command execution the source of truth for a project.

The project starts from a modest observation: generating code is only one part
of software engineering. A durable system also needs explicit product intent,
stable architecture, bounded component responsibilities, testable contracts,
controlled execution, independent evidence, and a person who can make the
final decision when those sources disagree.

## The problem Kvist addresses

AI assistance can shorten the path from an idea to an implementation. It can
also make important decisions difficult to inspect:

- requirements may exist only in a conversation;
- an agent may infer product or architecture decisions that nobody approved;
- broad context can create accidental coupling between components;
- execution permissions may be wider than the task requires;
- implementation success may be treated as evidence of correctness; and
- the same agent may effectively author, inspect, and approve its own work.

These are workflow and authority problems rather than shortcomings in any one
model. Kvist addresses them by making the engineering process explicit and
filesystem-native.

## What Kvist contributes

### Durable project authority

Vision, architecture, requirements, consumer contracts, design, task state,
and observed implementation behavior have distinct version-controlled homes.
The project does not depend on chat history to explain why work exists or what
consumers may rely on.

### Recursive component boundaries

Each component owns its local intent, execution plan, implementation, and
evidence. Context is deliberately narrow: local artifacts, global constraints,
and the immediate parent contract are available by default, while peer and
parent implementation details are not.

This structure is intended to reduce architectural drift without requiring one
large global prompt.

### Separation of intent from observation

Requirements and design describe what is intended. `IMPL.md` describes what
can be observed from source and test evidence in a separate clean-slate
context. A different reviewer compares those views without reading production
source.

Kvist does not claim that this process proves correctness. It creates a more
inspectable basis for review and makes disagreement visible.

### Bounded authority and explicit failure

External commands, repository content, model output, paths, configuration, and
schemas are treated as untrusted input. Execution-sensitive settings are
validated and bound to explicit policy. Limits, cancellation, redaction, and
durable evidence are part of the workflow rather than optional agent behavior.

The current host mode is not presented as a sandbox. Effectful task execution
requires a separately installed enforcement boundary with the expected
filesystem and network restrictions.

### Human arbitration

The human is not merely an approval button at the end of an autonomous
pipeline. A person owns product direction, architecture, exceptions, and final
arbitration.

AI review is advisory. A finding may be useful, mistaken, or overly strict. It
does not become correct because a model assigned it a high severity, and it
does not determine acceptance or compliance. When intent, implementation, and
evidence differ, Kvist retains that difference for an explicit human decision.

### Agent and provider independence

Kvist can use external coding agents and local model transports without making
their internal state authoritative. Provider frameworks such as Rig may supply
useful transport or loop mechanisms behind a Kvist-owned boundary, while
Kvist retains policy, authorization, durable state, and evidence semantics.

This allows tools to improve or be replaced without requiring the project's
engineering record to move with them.

## How this differs from adjacent efforts

Kvist overlaps with several useful categories of tools, but it is not intended
to replace them.

| Category | What it generally does well | Kvist's different concern |
| --- | --- | --- |
| Coding agents, including Copilot, Claude Code, Codex, Gemini, Aider, Goose, and OpenHands | Explore repositories, edit code, run tools, and complete implementation work | Define what authority an agent receives, preserve architecture and task state outside its conversation, and independently compare results with approved intent |
| Specification and planning kits, including Spec Kit, Kiro, OpenSpec, and BMad | Help create specifications, plans, and structured prompts | Maintain a recursive component lifecycle in which requirements, contracts, design, task state, implementation evidence, and arbitration have separate authority |
| Agent and model frameworks, including Rig | Provide model abstractions, tool calling, orchestration, and reusable loop machinery | Decide which context and effects are allowed, which evidence is canonical, and how uncertain or failed work affects durable project state |
| Sandboxes, containers, and managed runners | Restrict operating-system, filesystem, credential, and network access | Add semantic constraints: why work is authorized, which component contract applies, and whether observed results agree with intent |
| Policy, static-analysis, and governance platforms | Enforce organizational rules and report code or dependency findings | Connect project intent, constrained agent execution, observed implementation, and human arbitration in one version-controlled workflow |

These categories are complementary. A coding agent may perform the work, Rig
may carry a model request, a container may constrain effects, and a scanner may
produce a finding. Kvist's role is to keep those mechanisms subordinate to
explicit project authority and to retain evidence about what happened.

## When Kvist is likely to help

Kvist is most relevant when:

- architecture needs to remain understandable across many AI-assisted changes;
- components require narrow contracts and context boundaries;
- agent execution must be reviewable and reproducible;
- security or compliance work should not be self-certified;
- several agents or providers may be used over the life of a project; or
- a team wants human control without discarding automation.

It may be unnecessary for a short-lived prototype, a small personal script, or
work where conversational context and manual review are already sufficient.
Kvist deliberately adds structure, and that structure has a cost.

## Current maturity

Kvist is pre-release and currently supports Linux only. Its implemented CLI
already validates and records project artifacts, component state, task queues,
execution policy, bounded agent runs, and evidence. Some of the broader
workflow remains planned, including automated advisory review, project-level
acceptance, the complete Kvist-native tool loop, generalized provider-contract
materialization, and graphical interfaces.

The project reports these boundaries directly because reliability depends on
knowing what a tool does not yet guarantee. The aim is not maximum autonomy.
It is dependable, inspectable assistance under explicit human direction.

## Short description

> Kvist is a filesystem-native control and evidence layer for AI-assisted
> software development. It keeps architecture, component contracts, task
> authority, execution evidence, and human decisions in version-controlled
> project artifacts, so teams can use capable coding agents without making the
> agent the final authority.
