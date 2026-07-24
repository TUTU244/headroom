---
name: refactoring
description: Guides safe, behavior-preserving code refactoring — deduplicating repeated logic, improving names, splitting oversized functions/files, removing dead code, and untangling coupling — while keeping a test-backed safety net at every step. Use this whenever the user asks to refactor, clean up, simplify, restructure, "make this more maintainable," reduce duplication, split up a large function/file, improve naming, or remove unused/dead code, even if they don't use the word "refactor" explicitly (e.g. リファクタリング, コードの整理, 重複削除, 関数分割, 命名改善, デッドコード削除). Do not use this for one-off formatting/lint fixes or for changes that intentionally alter behavior (those are feature work or bug fixes, not refactoring).
---

# Refactoring

Refactoring changes the internal structure of code without changing its observable
behavior. The moment a change alters what the code *does*, it stops being a
refactor and becomes a feature or a bug fix — those need their own review and
testing standards, not this workflow. Keep the two separate: if a refactor
surfaces a real bug or a needed behavior change along the way, note it and
handle it as a distinct change (before or after), rather than folding it in.

## Why the process matters

Refactoring is risky precisely because "no behavior change" is a claim, not a
fact, until it's verified. The value of this skill is in the discipline around
that verification — small steps, a safety net, and reversibility — not in any
particular renaming or splitting technique. Skipping the discipline to move
faster is how refactors quietly turn into regressions.

## Workflow

### 1. Scope the change and check for a safety net

Before touching anything, get clear on what's in scope (a function, a file, a
module — not "the whole codebase") and what tests exist for it.

- If solid tests already cover the behavior, note the command to run them and
  move on.
- If coverage is thin or missing, write characterization tests first: tests
  that pin down the *current* observed behavior (including quirks and edge
  cases), not the behavior you wish it had. These are temporary scaffolding
  for the refactor, not necessarily the final test suite — though they're
  often worth keeping.
- If the code is impossible to test in its current form (e.g. a function
  tangled with I/O, globals, or hidden state), that tangle is itself a
  refactoring target — extracting a testable core may need to be step one.

Run the existing test suite once up front so you have a known-good baseline
before making any change.

### 2. Identify the smells, and name what you're fixing

Look for concrete, nameable problems rather than vague dissatisfaction:

- **Duplication** — the same logic (not just similar-looking code) copied
  across call sites. Before extracting, confirm the duplicates are actually
  meant to change together — coincidentally identical code that represents
  unrelated concepts should usually stay separate.
- **Poor naming** — names that require a comment to explain, that lie about
  what the thing does, or that are so generic (`data`, `handleStuff`,
  `temp2`) they convey nothing.
- **Oversized functions/files** — a function doing several unrelated things,
  or a file that's grown into a dumping ground. The signal isn't line count
  by itself, it's "can I describe this in one sentence without using 'and'?"
- **Dead code** — code with no live caller, unreachable branches, flags that
  are always the same value now, commented-out code left "just in case."
  Confirm it's actually unreachable (check for reflection, dynamic dispatch,
  external callers, feature flags) before deleting.
- **Coupling/structure issues** — modules that reach into each other's
  internals, circular dependencies, a class doing the job of three.

Write down the specific smells you're addressing before editing. This list is
also the natural table of contents for your commit messages and final
summary — don't lose it.

### 3. Make one kind of change at a time, in small verifiable steps

Pick a single smell from your list and fix it completely before moving to the
next. Interleaving "rename this AND extract that AND delete this other thing"
in one edit makes it much harder to tell which change broke a test if
something fails — and much harder for a reviewer to follow.

Within one smell, prefer the smallest step that's independently correct:

- Extract one function/module rather than restructuring the whole file at
  once.
- Rename in one coherent pass (definition + all call sites), don't leave a
  mix of old and new names.
- Delete dead code in its own step, separate from other structural changes,
  so a revert is trivial if something depended on it after all.

After each step: re-run the tests (and the characterization tests from step
1). If they pass, that step is done — commit it or otherwise checkpoint it
before starting the next. If they fail, the smallest recent change is almost
always the cause; don't stack another edit on top of a red test suite.

### 4. Preserve external behavior and interfaces unless the user asked otherwise

Public function signatures, API responses, CLI output, config formats, and
error messages are all "behavior" from a caller's point of view, even when
the internals change completely. If a refactor seems to require a breaking
change to an interface, stop and confirm that's actually in scope — it's easy
to smuggle a breaking change in under the label "refactor."

### 5. Wrap up: summarize what changed and why

When the refactor is complete, summarize it in terms of the smells identified
in step 2 — "extracted X to remove duplication between A and B," "renamed Y
because it no longer matched its behavior," "deleted the unused Z code path."
This is what makes the change reviewable: a reviewer should be able to check
each claim against the diff, not have to reverse-engineer intent from a
generic "refactored code" message.

If, during the work, you found something out of scope (a real bug, a
behavior change worth making, a smell too large to fix right now), call it
out explicitly rather than silently doing it or silently dropping it.

## Quick self-check before calling it done

- Did behavior actually stay the same? (Tests pass, and you can explain *why*
  each remaining diff is structural, not behavioral.)
- Could each commit/step stand alone and still leave the code working?
- Does every new name earn its keep — would a reader need fewer comments now,
  not more?
- Is anything left that looks like an accidental behavior change, a
  half-finished extraction, or dead code introduced by the refactor itself
  (e.g. an old function left behind after inlining its one caller)?
