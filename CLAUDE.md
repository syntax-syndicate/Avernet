# CLAUDE.md

Claude Code should follow the repository-wide instructions in `AGENTS.md`.

In particular, do not introduce Python `T | None` types unless `None` is an
intentional state in the contract; required values must remain non-optional.

Before changing Git hooks, module CI entrypoints, Singlebox orchestration,
acceptance E2E tests, coverage manifests, or coverage reporting, read and follow
the `Local Pre-Push And Singlebox Coverage` section in `AGENTS.md`. Treat that
section and the referenced scripts as one contract; update the documentation
when their behavior changes.

Keep this file intentionally small so agent instructions do not drift. Update
`AGENTS.md` when project-wide rules change.
