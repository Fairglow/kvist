# Writing and scheduling tasks

Start with the **outcome**, not the edits you expect an agent to make. Pick the
component that owns the outcome. If an idea crosses component boundaries, plan
one component at a time: agree on the relevant consumer contract first, then
plan separately for each affected component. Do not copy peer internals into
a task or create cross-queue execution dependencies.

## From an idea to ready work

```bash
# Draft and review work; does not change intent or run tasks.
kvist task plan storage --idea 'Show a distinct message when the work queue is empty.'

# After reading the preview, use the exact numbered response it prints.
kvist decision storage actions
kvist decision storage do 1  # Use the actual number shown for this plan.

# Review what is schedulable, then run it under existing execution policy.
kvist task next storage
kvist task run storage
```

The planner uses the configured architect model. It can propose exact local
intent edits when an idea needs a new requirement. Put non-goals, a known
reproduction, or an example of success in the idea when available. Edit and
restage the idea if the preview doesn't match what you want; a draft is not
scheduled work. The advisory review evaluates the candidate's intent and
task definitions, not source or implementation. To apply without a shortcut,
use the exact `kvist task apply COMPONENT PLAN_ID --approve SHA256` printed in
the preview. Applying does not run tasks or commit. If the inputs have changed,
stage and review a new plan; an approval never transfers to it automatically.

If work **already paused** for a decision, use `kvist decision COMPONENT`
instead. It shows the blocker and offers ways to discuss, revise, or stage
follow-up work. See the [command reference](command-set.md#45-the-interactive-tui-and-markdown-first-output).

## What makes a task ready?

| Include | Write it so a fresh agent can act |
| --- | --- |
| Outcome and scope | State one observable change. Name the component boundary, fixtures or files if known, and important non-goals. |
| Context and purpose | Explain the starting condition and why it matters. Link to evidence instead of pasting long logs or work history. |
| References and dependencies | Cite exact `DOCUMENT.md#LOCATOR` entries and only real prerequisites. Use local requirements, contract and design, the root contract, or the immediate parent contract. Prefer stable requirement IDs over broad headings. |
| Expected evidence | Name the fixture, exact check, or review deliverable and the result that counts. Include a positive case and a meaningful boundary case when relevant. “Tests pass” or “review complete” alone isn't enough. |

Use the four phases for distinct outcomes: **tests** demonstrate the starting
behavior or named regression; **implementation** satisfies the requirement
and rechecks the regression; **audit** records scoped independent findings
and gaps; **compliance review** compares observed behavior and verification
evidence against intent without source access. A completed report is not
a clean verdict or acceptance. Discrepancies requiring arbitration should
pause dependent work rather than be rewritten as passes.

Shared quality gates (approved formatting, lint, build and test policy;
independent review and human acceptance) are the common Definition of
Done. They do not replace the per-task evidence above. A task may be
schedulable by dependencies but still need a clearer brief.

Protected native authoring sees line hints in its selected intent files and a
bounded summary of direct prerequisites. A retained verification observation is
historical, not a fresh pass or a human acceptance decision.

## Local manual draft

`kvist task plan COMPONENT --import FILE` skips model drafting. Add
`--review-exception 'reason'` only for a deliberate, exact-plan review
exception; it also works offline without an architect model. A draft can
contain up to 16 ordered work items. It does not contain a whole queue,
timestamps, status, acceptance or recovery fields.

For a Rust component whose requirements already contain `REQ-EMPTY-QUEUE`,
save this as `draft.json`:

```json
{
  "schema_version": 1,
  "rationale": "Distinguish no work from work blocked by prerequisites.",
  "items": [{
    "id": "empty-queue-output",
    "title": "Distinguish empty and blocked queues",
    "context": "The status command currently uses one message for both cases.",
    "purpose": "Let an operator tell whether action is required.",
    "requirements": ["REQUIREMENTS.md#REQ-EMPTY-QUEUE"],
    "regression_tests": ["tests::empty_queue_has_distinct_message"],
    "write_tests": {
      "description": "Add the named renderer test. Do not change production code.",
      "expected_outcome": "The named case fails on the old renderer; the blocked fixture still passes."
    },
    "implement_code": {
      "description": "Change the empty-state renderer, leaving blocked-state output unchanged.",
      "expected_outcome": "The named case passes and the blocked fixture is unchanged."
    },
    "security_audit": {
      "description": "Review handling of queue-derived text in the two status branches.",
      "expected_outcome": "Record findings and coverage gaps with source or test evidence."
    },
    "compliance_review": {
      "description": "Compare the independent observation and test evidence to REQ-EMPTY-QUEUE.",
      "expected_outcome": "Record whether each display case is supported and identify any discrepancy."
    }
  }]
}
```

Use optional `depends_on` for existing or earlier generated task IDs. Use
optional `document_edits` entries with `path`, unique exact `before`, and
literal `after` when a clause needs to be added or revised. The planner
validates new locators in the materialized candidate; supported locators
are Markdown headings or visible `REQ-ID:`-style labels. It does not infer a
clause from an example in a code block or accept an ambiguous heading.

`regression_tests` is optional and Rust-specific. Name exact, unique libtest
cases (not shell commands, wildcards, or doctest labels). The host runs the
same approved quality pipeline: the test phase requires precisely those
failures, and implementation requires those cases to pass. Compilation,
infrastructure, timeout, cancellation, and excess-output failures don't
count. If another failing case masks the result, resolve or isolate that
failure instead of accepting it as the intended regression.

## Ownership and practices

Agents can draft and explain work, edit authorized implementation files, and
propose a decision when blocked. **Kvist** owns scheduling, status, timestamps,
verification records and recovery. **Humans** approve the exact plan and
accept, block, or reset supervised implementation attempts using
`kvist task finalize COMPONENT TASK_ID ATTEMPT_ID`. A manual `task transition`
asserts a queue state; it isn't a substitute for evidence-bound acceptance.

The checklist draws on [INVEST](https://agilealliance.org/glossary/invest/) for
small testable work, [Given/When/Then](https://cucumber.io/docs/gherkin/reference/)
for concrete examples, and the [Scrum Guide](https://scrumguides.org/scrum-guide.html)
for a shared Definition of Done. [Long-running agent harnesses](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents)
and [separate planning and evaluation](https://www.anthropic.com/research/harness-design-long-running-apps)
inform the bounded handoff and independent-review design. These are engineering
precedents, not a standards-conformance claim.
