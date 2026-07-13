# AGENTS.md

This file is the repository-wide instruction set for human contributors and AI
coding agents working on OCB.

## Source of Truth

Before changing code, read the files that define the boundary you are touching.
For architecture-sensitive work, these documents are mandatory:

- `docs/arch/arch.rules.md` — the OCB architecture constitution.
- `docs/arch/ci.enforce.md` — required CI gates for enforcing the constitution.
- `docs/arch/context-boundary-format.md` — required context boundary metadata
  for backend modules.
- `docs/arch/protocol-contract-tests.md` — required conformance test shape for
  plugin protocols.

If a module has its own `AGENTS.md`, follow it in addition to this file. The
more specific file controls only within that module.

## Project Overview

OCB is a monorepo for a multi-bot AI workbench. It includes:

- bot lifecycle management
- multi-bot coordination through BCS
- chat and runtime adapter services
- a frontend workbench
- contract and architecture validation documents

## Repository Layout

```text
ocb/
├── docs/
│   ├── arch/                 # Architecture constitution and CI constraints
│   └── open-source/          # Open-source readiness and design notes
├── scripts/                  # Local orchestration and utility scripts
├── src/
│   ├── backend/              # Python backend service
│   ├── frontend/             # TypeScript frontend workbench
│   ├── engine/               # Python engine adapter
│   ├── bcs/                  # Rust coordination service
│   └── tui/                  # Rust terminal client
└── tests/                    # Cross-module tests
```

## Module Responsibilities

| Module | Directory | Responsibility |
| --- | --- | --- |
| Frontend | `src/frontend/` | Web workbench UI |
| BCS | `src/bcs/` | Bot coordination, group chat, routing |


Entity ownership:

- Bot configuration, capabilities, and chat history belong to engine-facing
  services.
- User assets, bindings, and metadata belong to backend-facing services.
- Bot relationships, routing, and coordination belong to BCS.

## Architecture Rules

The architecture constitution is binding. In practice:

- Contracts are the authority for inter-component behavior.
- Service APIs and Plugin APIs must be defined, documented, versioned, and
  tested separately.
- Core logic must stay transport-agnostic.
- Delivery adapters translate protocol details; they do not own domain policy.
- Composition roots are the approved place to select concrete implementations.
- Raw environment access belongs in configuration loading, bootstrap,
  composition roots, or tests.
- Hardcoded external URLs, tokens, and private endpoints must not be introduced.
- Contract changes require matching docs and conformance or compatibility tests.
- Waivers for invariant violations must be explicit, owned, reviewed, and
  time-bounded.

## CI Expectations

Changes should preserve or improve the gates described in
`docs/arch/ci.enforce.md`:

- dependency boundary checks
- forbidden transport/framework usage in core
- restricted environment access
- configuration schema validation
- conformance test execution
- structural PR checklist completion
- red-flag detection for hardcoded URLs, tokens, and boundary drift

Do not weaken these checks to make a change pass. If a check is wrong, fix the
check and document why.

## Local Pre-Push And Singlebox Coverage

The repository-local pre-push gate is the local counterpart of required PR CI.
Its canonical entrypoints are:

- `.githooks/pre-push` for Git's pushed-ref protocol and comparison range
- `scripts/ci/pre_push.sh` for affected-system dispatch
- `src/<system>/scripts/ci_test.sh` for each system's test and coverage gate
- `scripts/ci/singlebox_coverage.sh` for live Singlebox E2E coverage

### Installation

Git does not enable versioned hooks automatically after clone. Install this
repository's hooks once per repository or worktree:

```bash
./scripts/install_git_hooks.sh
git config --get core.hooksPath  # expected: .githooks
```

Running `scripts/singlebox.sh` also installs the hook idempotently unless
`OCB_SKIP_GIT_HOOKS=1` is set. That variable only skips automatic installation;
it does not disable a hook that is already installed.

### Comparison And Dispatch

For each ref being pushed, `.githooks/pre-push` computes:

```text
base = merge-base(origin/dev, local_sha)
head = local_sha
changed files = git diff --name-only base head
```

This models a PR's complete delta from `dev`, rather than only the commits since
the last push to the feature branch. If `origin/dev` is unavailable, the hook
falls back to the remote branch SHA and then to the repository's first commit.
Only committed content in this range is checked; uncommitted working-tree
changes are outside a pre-push hook's input.

`scripts/ci/pre_push.sh` dispatches only the gates affected by those paths:

| Changed path | Required local gates |
| --- | --- |
| `src/backend/**` | Python SAST, Backend `ci_test.sh`, Singlebox coverage |
| `src/baas/**` | Python SAST, BaaS `ci_test.sh`, Singlebox coverage |
| `src/engine/**` | Python SAST, Engine `ci_test.sh` |
| `src/bcs/**` | BCS `ci_test.sh --fast-fail` |
| `src/frontend/**` | Frontend `ci_test.sh` |
| Singlebox orchestration or live E2E paths | Singlebox coverage |

Singlebox-triggering paths include `scripts/singlebox.sh`, `scripts/modules/**`,
`scripts/ci/singlebox_coverage.sh`, Backend acceptance tests, and BaaS E2E
tests. If several changed paths request Singlebox coverage, it runs only once
per `pre_push.sh` invocation.

The local Python SAST narrows its scan to added or modified Python files under
the affected system. This prevents violations in untouched files from blocking
a push, but it is file-granular, not line-granular: an existing violation in a
file changed by the branch can still be reported.

Useful direct checks are:

```bash
# Show the gates selected by the current branch without executing them.
scripts/ci/pre_push.sh \
  --base "$(git merge-base HEAD origin/dev)" --head HEAD --dry-run

# Execute the same local dispatcher directly.
scripts/ci/pre_push.sh \
  --base "$(git merge-base HEAD origin/dev)" --head HEAD
```

Singlebox coverage is required by default. For narrow local diagnosis only, it
can be skipped explicitly with either
`OCB_PRE_PUSH_SKIP_SINGLEBOX_COVERAGE=1 git push` or the dispatcher's
`--skip-singlebox-coverage` option. This bypass does not change required GitHub
PR checks and must not be used to claim Singlebox validation passed.

### Singlebox Coverage Execution

`scripts/ci/singlebox_coverage.sh` uses this flow:

```text
singlebox_coverage_modules.yaml
        |
        v
select enabled modules and deduplicate their acceptance targets
        |
        v
setup and start one real standalone Singlebox stack
        |
        v
run all selected live acceptance tests against that shared stack
        |
        v
combine Backend and BaaS process coverage plus Router/Plugin hit evidence
        |
        v
calculate and gate each module independently
        |
        v
write summary, JUnit, coverage, HTML, and dashboard artifacts
```

The stack and HTTP interactions are real. Model configuration uses mock values
so the coverage gate does not require external model credentials or claim that
model responses were validated.

With no module arguments, the runner selects every enabled module under
`modules` in `scripts/ci/singlebox_coverage_modules.yaml`. A focused run may
repeat `--module`:

```bash
# Run every enabled and registered module.
scripts/ci/singlebox_coverage.sh

# Run selected modules while still sharing one stack startup.
scripts/ci/singlebox_coverage.sh --module devices --module cron
```

For each module, the manifest owns:

- `acceptance_targets`: real user-story tests executed against the live stack
- `core_paths`: the production-code denominator for Core line coverage
- `router_api` and `plugin_api`: audited API denominators, or an explicit
  `not_applicable` reason while that denominator is unavailable
- `thresholds`: the non-regression floor enforced for that module

Any selected module below any applicable threshold fails the whole gate, but
all selected module errors are reported together. Modules under
`pending_modules` are visible migration debt and are neither executed nor
gated until their live scenarios are stable.

Artifacts are written below
`scripts/.dependencies/coverage/singlebox/reports/`, including:

- `acceptance-junit.xml` and `acceptance.log`
- Backend and BaaS coverage JSON, text, and HTML reports
- `summary.json` and `summary.md` with per-module metrics and threshold errors
- `dashboard.html` with the aggregated module view

### Adding A Coverage Module

When onboarding another module:

1. Add meaningful live acceptance user stories; do not add tests whose only
   purpose is touching lines.
2. Register the acceptance target and complete Core/Router/Plugin denominators
   in `singlebox_coverage_modules.yaml`.
3. Run the focused module against a freshly started standalone stack and set
   thresholds to its measured baseline.
4. Run the default all-module gate to catch shared-stack interference.
5. Add or update reporter and shell contract tests when manifest or runner
   behavior changes.

Do not inflate coverage by broadly excluding production Core paths. Coverage
instrumentation must remain at runtime, adapter, middleware, or composition
boundaries and must not leak test-only calls into domain Core code.

## Development Guidelines

Start from the requirement and the existing contract. Keep changes small and
traceable.

- Do not add features that were not requested.
- Do not add speculative abstraction or configurability.
- Do not refactor unrelated code.
- Match local style in the files you touch.
- Remove only dead imports, variables, or helpers caused by your own change.
- Propagate database and persistence write failures as errors; never silently
  swallow failed writes and return success.
- Prefer structured parsers and APIs over ad hoc string manipulation.
- Keep public-local development free of company-only services, registries,
  domains, credentials, and runtime state.

### Python Type Contracts

- Use `T | None` only when `None` is an intentional, valid state in the domain
  contract or at an external input boundary.
- Keep required configuration values, request fields, constructor arguments,
  and service method parameters non-optional end to end.
- Do not widen a required type to `T | None` merely for defensive programming,
  uncertain call sites, test convenience, or a fallback that hides missing
  input. Validate at the boundary and fail clearly instead.
- Before introducing an optional type, verify that a real caller can omit the
  value and that the receiving code defines meaningful behavior for `None`.

## Testing Rules

Choose tests based on risk:

- Narrow implementation change: run the closest unit tests.
- Contract change: update and run the relevant conformance tests.
- Cross-module behavior change: run all affected module tests.
- Architecture boundary change: run the architecture tests and update docs.
- BCS Rust changes: read `src/bcs/AGENTS.md` first, then run the relevant Cargo
  tests.

If you cannot run a required test, state exactly what was not run and why.

## Common Commands

```bash
# Root Python workspace
uv sync

# BCS
cd src/bcs
cargo test --workspace
```

Frontend public setup is still being finalized. Do not replace unresolved
public setup gaps with private registry or company-network assumptions.

## File Hygiene

Do not commit:

- secrets, tokens, cookies, or private keys
- local databases or runtime state
- generated logs and cache directories
- machine-specific agent configuration
- private service endpoints or private package indexes

Open-source defaults must be reproducible from public dependencies or clearly
marked as TODO.
