# Repository execution contract

## Rule precedence

Follow instructions in this order:

1. The latest `HUMAN → EXECUTOR` instruction.
2. The current `ORCHESTRATOR → EXECUTOR` handoff.
3. The GitHub issue and unresolved review feedback.
4. Applicable nested `AGENTS.md` files.
5. This root `AGENTS.md`.
6. Surrounding code and repository conventions.

Higher-priority instructions override lower-priority instructions. When no rule specifies a detail, preserve the conventions of the code being changed.

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

## Build and installation

- Installation is prohibited by default. Only an explicit current human or orchestrator instruction can authorize a named non-interactive build or installation action, and that authority applies only to the named action.
- Installation never authorizes application launch or interface control.
- Stop if installation requires unexpected privileges, GUI interaction, credentials, a security bypass, or destructive system changes.

## Human verification

- Report blocking human verification separately from deferred low-risk visual verification.
- Report whether installation is required and whether it succeeded.

## Delivery

- Keep one issue on one branch and one pull request. Include exactly one `Fixes #N` reference when required, and do not begin downstream work.
- Keep requested corrections on that issue's existing branch and pull request.
- Begin completion handoffs with `EXECUTOR → HUMAN` or `EXECUTOR → ORCHESTRATOR`, as requested.
- A completion handoff must state the repository, issue and PR, branch, latest commit, CI state, unresolved feedback, uncovered requirements, blocking and deferred human-verification needs, installation state, queue state, and any blocker.
- When installation succeeds, include the installed commit or version. When it fails, include the installation blocker.
