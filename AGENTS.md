# Repository execution contract

## Rule precedence

- Follow the active issue and user instructions first, then every applicable `AGENTS.md`, then the surrounding code.
- When rules do not specify a detail, preserve the conventions of the code being changed.

## Scope and design

- Build only what the issue requires. Do not add speculative options, hooks, abstractions, files, dependencies, cleanup, or formatting.
- Prefer direct, readable control flow and existing repository structures. Add an abstraction only for multiple real callers.
- Keep related code together. Use names that state intent, match one term to one concept, and remain honest about side effects.
- Keep functions at one level of abstraction. Comments explain non-obvious reasons, ordering, workarounds, retries, or timeouts; never restate code or retain commented-out code.
- Use repository formatters. Do not introduce unexplained constants, dead code, unused exports, or unreachable branches.

## Boundaries and errors

- Validate untrusted input at its boundary and fail with useful local context.
- Do not hide errors, weaken safety checks, or add handlers that only log and rethrow.
- Do not add or update a dependency without explicit human approval.

## Tests and validation

- Tests belong to the behavior being changed. Name the asserted behavior and expected result.
- Test observable behavior at the narrowest stable boundary with deterministic fixtures. Do not mock the unit under test or change a test merely to match current code.
- Preserve existing regressions and use repository-native headless commands for focused validation. Let the task instructions determine whether broader local or hosted checks are required.

## Machine safety

- Preserve unrelated worktree changes and avoid destructive commands or broad filesystem targets.
- Do not launch or interact with the application unless the task explicitly requires it. Do not use GUI, browser, Finder, AppleScript, simulated-input automation, or persistent services for validation.

## Delivery

- Keep one issue on one branch and one pull request. Include exactly one `Fixes #N` reference when required, and do not begin downstream work.
- An executor completion handoff must state the repository, issue and PR, branch, latest commit, CI state, unresolved feedback, uncovered requirements, human-verification need, queue state, and any blocker.
