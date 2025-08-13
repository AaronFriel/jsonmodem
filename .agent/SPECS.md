# ChatGPT Codex Execution Spec (ExecSpec) Writing Guide

Follow this guide when drafting **each** execution spec (ExecSpec) for ChatGPT Codex.

## Context

These rules ensure every ExecSpec can be executed by a stateless coding agent that knows nothing except the **current** repository contents *and* the **single ExecSpec file** it receives.

## Envelope & Formatting

Follow these instructions *for each* execution spec. An execution spec:
* Consists of exactly one top-level Markdown code block, beginning and ending with **triple backticks**.
* Contains **no internal code fences** (no additional triple backticks). Use **indentation** for code/transcripts inside the ExecSpec.
* Uses **four-space indentation** for nested code snippets.

## Audience Model

* The agent can `grep`, `ls`, `cat`, run tests, and execute commands, but it retains **zero memory of prior ExecSpecs**.
* **One agent → one ExecSpec → zero history.** Any context introduced earlier is invisible and **must be repeated** here.
* Do **not** require reading external papers, blogs, or online API docs; place all needed knowledge inside the ExecSpec.

## Content Checklist for Every ExecSpec

| Section Name                  | Mandatory Contents                                                                                                                                                                                                                                                                             |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Context**                   | One paragraph summarizing repo state **as of the end of the previous work** (repeat anything the agent must know).                                                                                                                                                                             |
| **Goal / Scope**              | Bullet list describing behavior to implement **in this ExecSpec only**.                                                                                                                                                                                                                        |
| **Definitions**               | Precise, code-level meaning for every new term or acronym.                                                                                                                                                                                                                                     |
| **File Map**                  | Table of full repo paths marked *create* / *modify*.                                                                                                                                                                                                                                           |
| **API & Data Structures**     | Full signatures **and** error enums; pin crate versions to avoid drift.                                                                                                                                                                                                                        |
| **Algorithms / Control Flow** | Deterministic, step-by-step pseudocode; specify tie-break rules for any ambiguity.                                                                                                                                                                                                             |
| **Tests**                     | Unit, property, snapshot, and acceptance checks for success **and** failure paths; include mocks for external I/O traits.                                                                                                                                                                      |
| **Milestones**                | Break the ExecSpec into one or more **Milestones**, each a small, testable unit: title, scope, acceptance checks, exact commands to run, and observable artifacts. Each milestone must be independently verifiable and enable incremental progress even if later milestones are not completed. |
| **Notes for the Implementer** | Edge cases, OS-specific fallbacks, RNG seeds, stub hints.                                                                                                                                                                                                                                      |
| **Next Work**                 | *Optional.* Brief glance at future work—no deep spec.                                                                                                                                                                                                                                          |
| **Handoff (Required if Partial)** | A **one-paragraph rule** + skeleton reference (see below): if time runs out or a blocker prevents completing all milestones, **stop** and return a **Short Handoff ExecSpec** (same format as this guide, but scoped only to the remaining work), including minimal artifacts (commands/exit codes; `git status`/`git diff --patch` or file list; failing test snippets) embedded as **indented blocks**. |

### Milestones (structure and requirements)

* Format each milestone as a clearly labeled subsection, for example: `### Milestone 1: Initialize CLI`.
* For each milestone, include:
  * **Scope**: exactly what changes to make.
  * **Commands**: the shell commands and test invocations to run.
  * **Acceptance**: pass/fail criteria and files that should change or be produced.
  * **Idempotence**: instructions must be safe to re-run without side effects.
  * **Rollback/Fallback**: how to revert or skip if a step fails, while preserving partial progress.
  * **State (for Handoff)**: mark **Completed / Partial / Blocked**.
* Milestones must be **totally ordered** and **testable** in isolation. Later milestones may depend only on artifacts produced by earlier milestones within the **same ExecSpec**.

## Handoff — When Time Runs Out (Keep This Short)

* **Stop criteria** (any one): cannot finish the **current milestone** with the remaining time; imminent timeout/token limits; failing build/tests with non-trivial fix.
* **What to return**: a **Short Handoff ExecSpec** that (a) restates Context with a “Completed so far” bullet list; (b) scopes **Goal / Scope** to only remaining work; (c) includes trimmed **File Map / APIs / Algorithms / Tests / Milestones** for the remainder; (d) embeds artifacts (commands + exit codes, diffs/status, failing test snippets) as **indented** transcripts.
* **No free-form prose**: the handoff itself is a single fenced ExecSpec block that another agent can execute immediately.

## Style Rules

* Write imperative instructions (“Implement…”, “Return…”)—never past tense.
* Expand every acronym the first time it appears.
* Route OS or network interaction through traits (for example, `CommandExecutor`) so tests can mock behavior.
* Prefer a pure-functional core; isolate side effects.
* Code fragments must compile in isolation; **indent rather than fence** them.
* Use Title Case headings (`Context`, `Goal / Scope`, etc.).
* Use `-` for main lists and `*` for sub-bullets for clear visual hierarchy.
* **On Partial Progress**: always end by returning either (a) completed outputs **or** (b) the **Short Handoff ExecSpec**.

## Sequencing Discipline

* **Within an ExecSpec**: Milestone **N** may depend only on artifacts fully specified and produced by milestones **≤ N** in this ExecSpec.
* If a concept is needed again, **restate it**—never rely on earlier files.
* Mention future ideas only in the final **Next Work** note.

## Self-Containment Mantra — Repeat Early & Often

1. An agent consumes **exactly one** ExecSpec file.
2. Everything required to compile, test, and reason **must live in that file**.
3. If you are tempted to say “as defined previously,” **copy the definition in**.

---

## Short Handoff ExecSpec — Minimal Skeleton (copy-paste)

```

# ChatGPT Codex Execution Spec (ExecSpec) — Handoff (Short Form)

## Context

```
- Completed so far (3–7 bullets tied to files/tests)
- Known failures or partial implementations
```

## Goal / Scope

```
- Remaining items only
```

## Definitions

```
- Only if new terms are required for the remaining work
```

## File Map

```
<path>    create|modify    <1-line reason>
```

## API & Data Structures

```
<only remaining deltas>
```

## Algorithms / Control Flow

```
<remaining pseudocode>
```

## Tests

```
- New/fixed tests
- Short excerpts of current failures and planned resolution
```

## Milestones

```
### Milestone N: <Title>
    Scope
        - ...
    Commands
        - ...
    Acceptance
        - ...
    Idempotence
        - ...
    Rollback/Fallback
        - ...
    State
        - Not Started / Partial / Blocked
```

## Notes for the Implementer

```
- Risks / TODOs
- Artifacts (embed as indented blocks, no extra fences):
    - Commands + exit codes
    - git status / git diff --patch (or file list + inline diffs)
    - Failing test snippets
```

## Next Work

```
- Optional hints
```
