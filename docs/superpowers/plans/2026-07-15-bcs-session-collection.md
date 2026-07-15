# BCS Session Collection (收藏) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a bot collect/uncollect sessions in a group and list the sessions it has collected, by adding a `collected` column to `bcs_session_participants` and three new methods (with default impls) on the session port/service.

**Architecture:** Collection is folded into existing session management (Approach B, B-ii). `SessionRepoPort` and `SessionManagementService` each gain `collect` / `uncollect` / `list_collected_by_group` with **default impls** (so the ~15 existing test mocks compile unchanged). Real impls live in `MySqlSessionStore`, `MemorySessionRepo`, and `bcs-session/src/application.rs`. HTTP adds a `collected` query filter on the existing list route and two new `/sessions/{sid}/collect` routes. The `Participant` domain struct and the `participants` JSON column are untouched — `collected` lives only in the side table (MySQL) / a `HashSet` (memory).

**Tech Stack:** Rust, axum, sqlx-free `bcs-db-api` (`DbPlugin`/`DbValue`/`DbStatement`), MySQL + SQLite (dual stores), `async_trait`. Workspace crate `bcs` under `src/bcs`.

## Global Constraints

- Repository-wide rules from `src/bcs/CLAUDE.md` and `AGENTS.md` apply. Do NOT run `cargo fmt`; keep whitespace edits to changed lines only.
- Do NOT introduce Python-style `T | None` for required values (this is Rust, but the spirit: keep required values non-optional in contracts).
- UTF-8 string safety: never slice strings by byte index for multibyte content (not directly relevant here, but obey project rule).
- Column type for booleans is `tinyint(4) NOT NULL DEFAULT '0'` (matches `is_creator`/`is_deleted`/`disabled`). SQLite equivalent: `INTEGER NOT NULL DEFAULT 0`.
- New port/service methods MUST have default impls (returning `Err` / empty vec) — do not change existing `list_by_group` signatures.
- `collected` MUST NOT be added to the `Participant` domain struct or the `participants` JSON column.
- The `participant` filter is required when `collected=true` (400 otherwise).
- uncollect is idempotent: its only 404 is session-not-found; non-participant / not-collected → 200.
- Pre-push target contract from `AGENTS.md`: default merge target `origin/dev`. Do not push unless asked.
- All cargo commands run from `src/bcs` unless noted.

---

## File Structure

**Create:**
- `src/bcs/migrations/mysql/004_add_session_collection.sql` — MySQL ALTER + index.
- `src/bcs/crates/adapters/http/bcs-http/tests/session_collection_contract.rs` — HTTP contract tests.

**Modify:**
- `src/bcs/crates/service-api/bcs-service-api/src/port/repo/session.rs` — add 3 methods + default impls to `SessionRepoPort`.
- `src/bcs/crates/service-api/bcs-service-api/src/application/session.rs` — add 3 methods + default impls to `SessionManagementService`.
- `src/bcs/crates/services/bcs-session/src/application.rs` — real impls delegating to repo.
- `src/bcs/crates/services/bcs-session/src/noop.rs` — noop impl (default-impl inherits, but noop is a direct impl; verify).
- `src/bcs/crates/services/bcs-session-store/src/mysql.rs` — `collect`/`uncollect`/`list_collected_by_group` real impls + SELECT column list unchanged.
- `src/bcs/crates/services/bcs-session-store/src/memory.rs` — side `HashSet` + 3 real impls.
- `src/bcs/crates/services/bcs-channel/src/lib.rs` — `RecordingSessionRepo` uses default impls (verify no override needed; may need nothing).
- `src/bcs/crates/bootstrap/bcs/src/migrations.rs` — SQLite CREATE TABLE column, `ensure_sqlite_session_collected_column`, version-4 entry + dispatch.
- `src/bcs/crates/test-support/bcs-test-support/src/contract/repo/mod.rs` — extend `session_repo_contract_tests` harness.
- `src/bcs/crates/adapters/http/bcs-http/src/routes/sessions.rs` — `collected` query field + branch + `collect_session`/`uncollect_session` handlers.
- `src/bcs/crates/adapters/http/bcs-http/src/router.rs` — register `/sessions/{sid}/collect`.
- `src/bcs/crates/adapters/http/bcs-http/tests/service_routes_contract.rs` and other mock `SessionManagementService` impls — only if they are `impl` blocks that need the new methods (default impls should make this unnecessary; each task verifies with a compile check).

---

## Task 1: Port + service trait methods with default impls

Add the contract surface first. With default impls, nothing else compiles differently yet, so this lands the names all later tasks depend on.

**Files:**
- Modify: `src/bcs/crates/service-api/bcs-service-api/src/port/repo/session.rs`
- Modify: `src/bcs/crates/service-api/bcs-service-api/src/application/session.rs`

**Interfaces:**
- Produces (port): `SessionRepoPort::collect(&self, session_id: &str, bot_uuid: &str) -> ServiceResult<()>`, `uncollect(...)` same sig, `list_collected_by_group(&self, group_id: &str, bot_uuid: &str, status: Option<SessionStatus>, title_contains: Option<&str>, offset: u64, limit: u64) -> Vec<Session>`.
- Produces (service): `SessionManagementService::collect(&self, session_id: &str, bot_uuid: &str) -> Result<(), SessionUseCaseError>`, `uncollect(...)` same sig, `list_collected_by_group(...)` returning `Result<Vec<Session>, SessionUseCaseError>`.

- [ ] **Step 1: Add default-impl methods to `SessionRepoPort`**

In `src/bcs/crates/service-api/bcs-service-api/src/port/repo/session.rs`, append inside the `SessionRepoPort` trait body (after `async fn delete`), before the closing `}`:

```rust
    // ── session collection (收藏) ──────────────────────────────
    // Default impls keep existing test mocks compiling; real impls in
    // mysql + memory override these (see bcs-session-store).
    async fn collect(&self, _session_id: &str, _bot_uuid: &str) -> ServiceResult<()> {
        Err(ServiceError::InternalError(
            "collect not implemented for this SessionRepoPort".into(),
        ))
    }
    async fn uncollect(&self, _session_id: &str, _bot_uuid: &str) -> ServiceResult<()> {
        Err(ServiceError::InternalError(
            "uncollect not implemented for this SessionRepoPort".into(),
        ))
    }
    async fn list_collected_by_group(
        &self,
        _group_id: &str,
        _bot_uuid: &str,
        _status: Option<SessionStatus>,
        _title_contains: Option<&str>,
        _offset: u64,
        _limit: u64,
    ) -> Vec<Session> {
        Vec::new()
    }
```

`ServiceResult`, `Session`, `SessionStatus` are already imported at the top of this file (verified: `use crate::types::{...}`). Confirm `ServiceError` is reachable — `ServiceResult` is `Result<T, ServiceError>`; if `ServiceError` is not in scope, import it via `crate::core::ServiceError`.

- [ ] **Step 2: Add default-impl methods to `SessionManagementService`**

In `src/bcs/crates/service-api/bcs-service-api/src/application/session.rs`, append inside the `SessionManagementService` trait body (after `async fn delete`), before the closing `}`:

```rust
    // ── session collection (收藏) ──────────────────────────────
    /// Mark a session as collected by `bot_uuid`. The bot must be a
    /// participant of the session (the side-table row exists).
    async fn collect(
        &self,
        _session_id: &str,
        _bot_uuid: &str,
    ) -> Result<(), SessionUseCaseError> {
        Err(SessionUseCaseError::Internal(ServiceError::InternalError(
            "collect not implemented for this SessionManagementService".into(),
        )))
    }
    /// Remove the collection mark. Idempotent: only session-not-found is an
    /// error; non-participant / not-collected returns Ok.
    async fn uncollect(
        &self,
        _session_id: &str,
        _bot_uuid: &str,
    ) -> Result<(), SessionUseCaseError> {
        Err(SessionUseCaseError::Internal(ServiceError::InternalError(
            "uncollect not implemented for this SessionManagementService".into(),
        )))
    }
    async fn list_collected_by_group(
        &self,
        _group_id: &str,
        _bot_uuid: &str,
        _status: Option<SessionStatus>,
        _title_contains: Option<&str>,
        _offset: u64,
        _limit: u64,
    ) -> Result<Vec<Session>, SessionUseCaseError> {
        Ok(Vec::new())
    }
```

`ServiceError`, `Session`, `SessionStatus` are already imported in this file (verified: `use crate::core::ServiceError;` and `use crate::types::{Participant, ParticipantMode, Session, SessionStatus};`).

- [ ] **Step 3: Verify the workspace still compiles**

Run: `cd src/bcs && cargo check --workspace`
Expected: PASS (default impls mean no other impl needs changes).

- [ ] **Step 4: Commit**

```bash
git add src/bcs/crates/service-api/bcs-service-api/src/port/repo/session.rs src/bcs/crates/service-api/bcs-service-api/src/application/session.rs
git commit -m "feat(bcs): add collect/uncollect/list_collected to session port+service with default impls"
```

---

## Task 2: Migration — MySQL `collected` column + index

**Files:**
- Create: `src/bcs/migrations/mysql/004_add_session_collection.sql`

**Interfaces:**
- Produces: DB column `bcs_session_participants.collected tinyint(4)` and index `idx_collected`.

- [ ] **Step 1: Write the migration file**

`src/bcs/migrations/mysql/004_add_session_collection.sql`:

```sql
-- Session collection (收藏): per-participant collected flag on the side table.
ALTER TABLE `bcs_session_participants`
  ADD COLUMN IF NOT EXISTS `collected` tinyint(4) NOT NULL DEFAULT '0';

CREATE INDEX `idx_collected`
  ON `bcs_session_participants` (`env`, `group_id`, `bot_uuid`, `collected`);
```

`ADD COLUMN IF NOT EXISTS` matches the existing `002_add_owner_bot_id.sql` idiom (verified).

- [ ] **Step 2: Commit**

```bash
git add src/bcs/migrations/mysql/004_add_session_collection.sql
git commit -m "feat(bcs): add collected column migration for session collection"
```

(No runnable test for a SQL migration in this task; MySQL ALTER is exercised end-to-end via Task 7 conformance once a MySQL test backend exists. The SQLite path — which IS runnable in CI — is covered in Task 3 with real tests.)

---

## Task 3: Migration — SQLite new-DB column + existing-DB ensure function + version 4

This closes the SQLite existing-DB gap: a bare version-4 record is a no-op, so we add an `ensure_sqlite_session_collected_column` function (mirroring `ensure_sqlite_message_owner_bot_id`) called from bootstrap, plus the new-DB column and version-4 dispatch.

**Files:**
- Modify: `src/bcs/crates/bootstrap/bcs/src/migrations.rs`

**Interfaces:**
- Produces: new-DB `bcs_session_participants.collected`, `ensure_sqlite_session_collected_column(db)` called in `run_sqlite_bootstrap_tables`, and `SqliteMigration { version: 4, name: "add_session_collection" }` with a `4 => Ok(())` dispatch.

- [ ] **Step 1: Write a test asserting the column appears on a fresh+existing SQLite DB**

The bootstrap crate has no existing migration unit-test file we should mirror blindly; instead add a focused test in the bootstrap crate's test module. First locate the test module:

Run: `cd src/bcs && grep -n "#\[cfg(test)\]\|mod tests" crates/bootstrap/bcs/src/migrations.rs | head`

If a test module exists, append into it; if not, add one at the bottom of `migrations.rs`. Add this test (TDD — it will FAIL until the ensure function + column exist):

```rust
#[cfg(test)]
mod collection_migration_tests {
    use super::*;
    use bcs_db_api::{InMemoryDbPlugin, DbStatement};

    async fn fresh_db() -> InMemoryDbPlugin {
        let db = InMemoryDbPlugin::default();
        run_sqlite_bootstrap_tables(&db).await.expect("bootstrap");
        run_sqlite_versioned_migrations(&db).await.expect("versioned");
        db
    }

    #[tokio::test]
    async fn fresh_db_has_session_participants_collected_column() {
        let db = fresh_db().await;
        let cols = sqlite_table_columns(&db, "bcs_session_participants").await.unwrap();
        assert!(cols.iter().any(|c| c == "collected"),
            "bcs_session_participants must have a collected column on fresh DB; got {cols:?}");
    }

    #[tokio::test]
    async fn ensure_function_adds_collected_to_legacy_table() {
        // Simulate a legacy DB: create the table WITHOUT the collected column,
        // then run bootstrap which should add it via the ensure function.
        let db = InMemoryDbPlugin::default();
        db.execute(DbStatement::new(
            "CREATE TABLE bcs_session_participants (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                gmt_create TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                gmt_modified TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                session_id TEXT NOT NULL,
                group_id TEXT NOT NULL,
                bot_uuid TEXT NOT NULL,
                role TEXT NOT NULL,
                env TEXT NOT NULL DEFAULT 'prod'
            )"
        )).await.unwrap();
        run_sqlite_bootstrap_tables(&db).await.expect("bootstrap repairs legacy table");
        let cols = sqlite_table_columns(&db, "bcs_session_participants").await.unwrap();
        assert!(cols.iter().any(|c| c == "collected"),
            "ensure function must add collected to legacy bcs_session_participants; got {cols:?}");
    }
}
```

> If `InMemoryDbPlugin` is not the right concrete type exported by `bcs-db-api`, run `cd src/bcs && grep -rn "impl DbPlugin" crates/plugins/bcs-db-local/src crates/plugin-api/bcs-db-api/src` to find the in-memory test plugin name and substitute it. `sqlite_table_columns` and `table_exists` are private fns in this same file (verified), so the test must live in this file's test module to call them.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src/bcs && cargo test -p bcs --lib collection_migration_tests -- --nocapture`
Expected: FAIL (no `collected` column, no ensure function).

- [ ] **Step 3: Add the column to the SQLite CREATE TABLE for `bcs_session_participants`**

In `migrations.rs`, find the `bcs_session_participants` CREATE TABLE statement in the bootstrap DDL list and add the column before the closing `)`:

```sql
        collected INTEGER NOT NULL DEFAULT 0,
```

(Preserve surrounding formatting — do not reformat other lines.)

- [ ] **Step 4: Add `ensure_sqlite_session_collected_column` and call it from bootstrap**

Add the function next to `ensure_sqlite_message_owner_bot_id` (mirror its exact shape, using `sqlite_table_columns`):

```rust
async fn ensure_sqlite_session_collected_column(db: &dyn DbPlugin) -> DbResult<()> {
    let columns = sqlite_table_columns(db, "bcs_session_participants").await?;
    if !columns.iter().any(|column| column == "collected") {
        db.execute(DbStatement::new(
            "ALTER TABLE bcs_session_participants ADD COLUMN collected INTEGER NOT NULL DEFAULT 0",
        ))
        .await?;
    }
    db.execute(DbStatement::new(
        "CREATE INDEX IF NOT EXISTS idx_collected \
         ON bcs_session_participants(env, group_id, bot_uuid, collected)",
    ))
    .await?;
    Ok(())
}
```

Then, at the end of `run_sqlite_bootstrap_tables` (where `ensure_sqlite_message_owner_bot_id(db).await?` is called — verified at `migrations.rs:775`), add right after that call:

```rust
    ensure_sqlite_session_collected_column(db).await?;
```

Also guard against the legacy table not existing:

```rust
    ensure_sqlite_session_collected_column(db).await?;
```

`ensure_sqlite_session_collected_column` calls `sqlite_table_columns` which runs `PRAGMA table_info(...)` — if the table does not exist, PRAGMA returns no rows and the ALTER would fail. Wrap the ALTER so a missing table is a no-op (the new-DB CREATE TABLE already creates it with the column):

```rust
async fn ensure_sqlite_session_collected_column(db: &dyn DbPlugin) -> DbResult<()> {
    if !table_exists(db, "bcs_session_participants").await? {
        return Ok(());
    }
    let columns = sqlite_table_columns(db, "bcs_session_participants").await?;
    if !columns.iter().any(|column| column == "collected") {
        db.execute(DbStatement::new(
            "ALTER TABLE bcs_session_participants ADD COLUMN collected INTEGER NOT NULL DEFAULT 0",
        ))
        .await?;
    }
    db.execute(DbStatement::new(
        "CREATE INDEX IF NOT EXISTS idx_collected \
         ON bcs_session_participants(env, group_id, bot_uuid, collected)",
    ))
    .await?;
    Ok(())
}
```

(Use this guarded version; `table_exists` is the private fn at `migrations.rs:986`.)

- [ ] **Step 5: Register version 4 + dispatch**

In `SQLITE_VERSIONED_MIGRATIONS` (verified at `migrations.rs:664`), append:

```rust
    SqliteMigration {
        version: 4,
        name: "add_session_collection",
    },
```

In `apply_sqlite_migration_body` (verified at `migrations.rs:849`), add a `4 =>` arm mirroring version 3 (the real repair is done by the ensure function in bootstrap; the version just records progress):

```rust
    match migration.version {
        2 => repair_sqlite_channel_bindings_audit_schema(db).await,
        // Startup creates any missing organization tables before recording version 3.
        3 => Ok(()),
        // collected column is added by ensure_sqlite_session_collected_column in
        // run_sqlite_bootstrap_tables; version 4 only records progress.
        4 => Ok(()),
        _ => Ok(()),
    }
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cd src/bcs && cargo test -p bcs --lib collection_migration_tests -- --nocapture`
Expected: PASS (both tests green).

- [ ] **Step 7: Commit**

```bash
git add src/bcs/crates/bootstrap/bcs/src/migrations.rs
git commit -m "feat(bcs): SQLite migration for session collection column (new + existing DBs)"
```

---

## Task 4: Memory store — `collect` / `uncollect` / `list_collected_by_group`

TDD the in-memory backend first (fast, no DB). It uses a `HashSet<(String, String)>` of `(session_id, bot_uuid)` held in `MemoryState`.

**Files:**
- Modify: `src/bcs/crates/services/bcs-session-store/src/memory.rs`

**Interfaces:**
- Consumes: `SessionRepoPort::collect/uncollect/list_collected_by_group` (Task 1).
- Produces: working memory impls used by the contract harness (Task 7) and HTTP contract tests (Task 9).

- [ ] **Step 1: Write failing unit tests**

In the `#[cfg(test)]` module of `memory.rs` (tests already exist there, e.g. `list_by_group_participant_filter`), add:

```rust
    #[tokio::test]
    async fn collection_collect_then_list_then_uncollect() {
        let repo = MemorySessionRepo::new();
        let gid = "col-group";
        let sess = repo
            .create(
                gid,
                NewSessionParams {
                    session_kind: SessionKind::Chat,
                    participants: vec![Participant::bot("bot1", ParticipantRole::Driver)],
                    ..Default::default()
                },
            )
            .await
            .expect("create");

        // not collected yet
        let listed = repo.list_collected_by_group(gid, "bot1", None, None, 0, 10).await;
        assert!(listed.is_empty());

        repo.collect(&sess.id, "bot1").await.expect("collect");
        let listed = repo.list_collected_by_group(gid, "bot1", None, None, 0, 10).await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, sess.id);

        // other bot does not see bot1's collection
        let other = repo.list_collected_by_group(gid, "bot2", None, None, 0, 10).await;
        assert!(other.is_empty());

        repo.uncollect(&sess.id, "bot1").await.expect("uncollect");
        let listed = repo.list_collected_by_group(gid, "bot1", None, None, 0, 10).await;
        assert!(listed.is_empty());
    }

    #[tokio::test]
    async fn collection_non_participant_collect_errors() {
        let repo = MemorySessionRepo::new();
        let gid = "col-group2";
        let sess = repo
            .create(
                gid,
                NewSessionParams {
                    session_kind: SessionKind::Chat,
                    participants: vec![Participant::bot("bot1", ParticipantRole::Driver)],
                    ..Default::default()
                },
            )
            .await
            .expect("create");
        let err = repo.collect(&sess.id, "not-a-participant").await;
        assert!(err.is_err(), "collect by non-participant must error");
    }

    #[tokio::test]
    async fn collection_uncollect_idempotent_for_non_participant() {
        let repo = MemorySessionRepo::new();
        let gid = "col-group3";
        let sess = repo
            .create(
                gid,
                NewSessionParams {
                    session_kind: SessionKind::Chat,
                    participants: vec![Participant::bot("bot1", ParticipantRole::Driver)],
                    ..Default::default()
                },
            )
            .await
            .expect("create");
        // uncollect a never-collected, still-participant session -> Ok
        repo.uncollect(&sess.id, "bot1").await.expect("uncollect not collected ok");
        // uncollect a non-participant -> Ok (idempotent)
        repo.uncollect(&sess.id, "nobody").await.expect("uncollect non-participant ok");
    }

    #[tokio::test]
    async fn collection_respects_status_and_title_filter() {
        let repo = MemorySessionRepo::new();
        let gid = "col-group4";
        let s_running = repo
            .create(gid, NewSessionParams {
                session_kind: SessionKind::Chat,
                session_title: Some("Alpha Report".into()),
                participants: vec![Participant::bot("bot1", ParticipantRole::Driver)],
                ..Default::default()
            })
            .await
            .expect("create");
        let s_to_complete = repo
            .create(gid, NewSessionParams {
                session_kind: SessionKind::Chat,
                session_title: Some("Beta Note".into()),
                participants: vec![Participant::bot("bot1", ParticipantRole::Driver)],
                ..Default::default()
            })
            .await
            .expect("create");
        repo.complete_if_running(&s_to_complete.id, None, None).await.expect("complete");
        repo.collect(&s_running.id, "bot1").await.expect("collect running");
        repo.collect(&s_to_complete.id, "bot1").await.expect("collect completed");

        let only_running = repo.list_collected_by_group(
            gid, "bot1", Some(SessionStatus::Running), None, 0, 10,
        ).await;
        assert_eq!(only_running.len(), 1);
        assert_eq!(only_running[0].id, s_running.id);

        let only_alpha = repo.list_collected_by_group(
            gid, "bot1", None, Some("alpha"), 0, 10,
        ).await;
        assert_eq!(only_alpha.len(), 1);
        assert_eq!(only_alpha[0].id, s_running.id);
    }

    #[tokio::test]
    async fn collection_lost_when_participant_removed() {
        let repo = MemorySessionRepo::new();
        let gid = "col-group5";
        let sess = repo
            .create(gid, NewSessionParams {
                session_kind: SessionKind::Chat,
                participants: vec![Participant::bot("bot1", ParticipantRole::Driver)],
                ..Default::default()
            })
            .await
            .expect("create");
        repo.collect(&sess.id, "bot1").await.expect("collect");
        assert_eq!(
            repo.list_collected_by_group(gid, "bot1", None, None, 0, 10).await.len(),
            1
        );
        repo.remove_participant(&sess.id, "bot1").await.expect("remove");
        // after leaving, collection mark is gone (memory set must be pruned)
        assert!(repo
            .list_collected_by_group(gid, "bot1", None, None, 0, 10)
            .await
            .is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src/bcs && cargo test -p bcs-session-store --lib collection_ -- --nocapture`
Expected: FAIL (methods use default impls; collect returns `InternalError`, list returns empty).

- [ ] **Step 3: Add `collected` set to `MemoryState`**

In `memory.rs`, update the state struct (verified at line 45):

```rust
struct MemoryState {
    sessions: HashMap<String, Session>,
    collected: std::collections::HashSet<(String, String)>,
}
```

Since `MemoryState` is constructed via `#[derive(Default)]` through `MemorySessionRepo::default()`, the `HashSet` field auto-defaults to empty — no manual constructor changes needed. (If `MemoryState` has an explicit `Default` impl rather than derive, add the field there too; check: `grep -n "impl Default for MemoryState" memory.rs`.)

- [ ] **Step 4: Implement `collect`, `uncollect`, `list_collected_by_group` on `MemorySessionRepo`**

Add these inside the `impl SessionRepoPort for MemorySessionRepo` block (after `delete`):

```rust
    async fn collect(&self, session_id: &str, bot_uuid: &str) -> ServiceResult<()> {
        let mut st = self.state.write().await;
        let session = st
            .sessions
            .get(session_id)
            .ok_or_else(|| ServiceError::SessionNotFound(session_id.to_string()))?;
        if !session.participants.iter().any(|p| p.bot_uuid == bot_uuid) {
            return Err(ServiceError::SessionNotFound(format!(
                "participant {bot_uuid} not in session {session_id}"
            )));
        }
        st.collected.insert((session_id.to_string(), bot_uuid.to_string()));
        Ok(())
    }

    async fn uncollect(&self, session_id: &str, bot_uuid: &str) -> ServiceResult<()> {
        let mut st = self.state.write().await;
        // Idempotent: session must exist; otherwise no-op removal.
        if !st.sessions.contains_key(session_id) {
            return Err(ServiceError::SessionNotFound(session_id.to_string()));
        }
        st.collected.remove(&(session_id.to_string(), bot_uuid.to_string()));
        Ok(())
    }

    async fn list_collected_by_group(
        &self,
        group_id: &str,
        bot_uuid: &str,
        status: Option<SessionStatus>,
        title_contains: Option<&str>,
        offset: u64,
        limit: u64,
    ) -> Vec<Session> {
        let st = self.state.read().await;
        let q = title_contains.map(|s| s.to_ascii_lowercase());
        let mut v: Vec<_> = st
            .sessions
            .values()
            .filter(|s| s.group_id == group_id)
            .filter(|s| {
                st.collected
                    .contains(&(s.id.clone(), bot_uuid.to_string()))
            })
            .filter(|s| status.map(|want| s.status == want).unwrap_or(true))
            .filter(|s| {
                q.as_ref().map_or(true, |q| {
                    s.session_title
                        .as_deref()
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .contains(q)
                })
            })
            .cloned()
            .collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        v.into_iter().skip(offset as usize).take(limit as usize).collect()
    }
```

- [ ] **Step 5: Prune collected entries in `remove_participant` and `delete`**

In `MemorySessionRepo::remove_participant` (verified), after the `retain`/write that drops the participant, also remove the collection mark. Find the spot where the participant is removed from `current.participants` and the state is written back; add inside the same write-lock section:

```rust
        // collection mark is per-participant; leaving drops it
        st.collected
            .retain(|(sid, bid)| !(sid == session_id && bid == bot_uuid));
```

(Adjust variable names to match that function's locals — `session_id` and `bot_uuid` are the fn params there, but `st` may be `self.state.write().await`; reuse the existing lock handle rather than re-acquiring. If the function already holds `current` in memory and writes via a separate lock, prune right where the participant is retained out, under the same lock.)

In `MemorySessionRepo::delete` (verified to return `ServiceResult<bool>`), after successfully removing the session from the map, prune all collection marks for that session:

```rust
        st.collected.retain(|(sid, _)| sid != session_id);
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd src/bcs && cargo test -p bcs-session-store --lib collection_ -- --nocapture`
Expected: PASS (all 5 collection tests green).

- [ ] **Step 7: Commit**

```bash
git add src/bcs/crates/services/bcs-session-store/src/memory.rs
git commit -m "feat(bcs): memory store collect/uncollect/list_collected_by_group"
```

---

## Task 5: MySQL store — `collect` / `uncollect` / `list_collected_by_group`

**Files:**
- Modify: `src/bcs/crates/services/bcs-session-store/src/mysql.rs`

**Interfaces:**
- Consumes: `SessionRepoPort` new methods (Task 1), `bcs_session_participants.collected` column (Tasks 2–3).
- Produces: MySQL impls used by conformance (Task 7).

- [ ] **Step 1: Implement `collect` and `uncollect`**

Add inside `impl SessionRepoPort for MySqlSessionStore` (after `delete`):

```rust
    async fn collect(&self, session_id: &str, bot_uuid: &str) -> ServiceResult<()> {
        let sql = "UPDATE bcs_session_participants \
                   SET collected = 1 \
                   WHERE env = ? AND session_id = ? AND bot_uuid = ?";
        let result = self
            .db
            .execute(DbStatement::with_params(
                sql,
                vec![
                    DbValue::from(self.env.as_str()),
                    DbValue::from(session_id),
                    DbValue::from(bot_uuid),
                ],
            ))
            .await
            .map_err(|e| ServiceError::InternalError(format!("session db: {e}")))?;
        if result.affected_rows == 0 {
            return Err(ServiceError::SessionNotFound(format!(
                "participant {bot_uuid} not in session {session_id}"
            )));
        }
        Ok(())
    }

    async fn uncollect(&self, session_id: &str, bot_uuid: &str) -> ServiceResult<()> {
        // Idempotent: the only caller-facing error is session-not-found, which
        // the application layer checks via get() before calling. Here we run the
        // UPDATE regardless of whether a side-table row / collected flag exists.
        let sql = "UPDATE bcs_session_participants \
                   SET collected = 0 \
                   WHERE env = ? AND session_id = ? AND bot_uuid = ?";
        self.db
            .execute(DbStatement::with_params(
                sql,
                vec![
                    DbValue::from(self.env.as_str()),
                    DbValue::from(session_id),
                    DbValue::from(bot_uuid),
                ],
            ))
            .await
            .map_err(|e| ServiceError::InternalError(format!("session db: {e}")))?;
        Ok(())
    }
```

`DbValue`, `DbStatement`, `ServiceError`, `ServiceResult` are already imported in this file (verified at the top of `mysql.rs`).

- [ ] **Step 2: Implement `list_collected_by_group`**

Add a private column-list helper is not needed — reuse `self.select_cols_prefixed()` (verified at `mysql.rs:68`). Add the method:

```rust
    async fn list_collected_by_group(
        &self,
        group_id: &str,
        bot_uuid: &str,
        status: Option<SessionStatus>,
        title_contains: Option<&str>,
        offset: u64,
        limit: u64,
    ) -> Vec<Session> {
        let mut conditions: Vec<String> = vec![
            "s.env = ?".to_string(),
            "s.group_id = ?".to_string(),
            "sp.group_id = ?".to_string(),
            "sp.bot_uuid = ?".to_string(),
            "sp.collected = 1".to_string(),
        ];
        let mut params: Vec<DbValue> = vec![
            DbValue::from(self.env.as_str()),
            DbValue::from(group_id),
            DbValue::from(group_id),
            DbValue::from(bot_uuid),
        ];

        if let Some(s) = status {
            let status_str = match s {
                SessionStatus::Running => "running",
                SessionStatus::Completed => "completed",
            };
            conditions.push("s.status = ?".to_string());
            params.push(DbValue::from(status_str));
        }

        if let Some(q) = title_contains {
            conditions.push("s.session_title LIKE ?".to_string());
            params.push(DbValue::from(format!("%{}%", q)));
        }

        params.push(DbValue::U64(limit));
        params.push(DbValue::U64(offset));

        let select_cols = self.select_cols_prefixed();
        let sql = format!(
            "SELECT {select_cols} FROM bcs_group_sessions s \
             JOIN bcs_session_participants sp \
               ON sp.env = s.env AND sp.session_id = s.session_id \
             WHERE {where_clause} \
             ORDER BY s.gmt_create DESC LIMIT ? OFFSET ?",
            select_cols = select_cols,
            where_clause = conditions.join(" AND "),
        );

        let rows = match self.db.query(DbStatement::with_params(&sql, params)).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "list_collected_by_group query failed");
                return Vec::new();
            }
        };
        rows.iter().filter_map(|r| row_to_session(r).ok()).collect()
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd src/bcs && cargo check -p bcs-session-store`
Expected: PASS.

- [ ] **Step 4: Commit**

(MySQL behavior is validated by the conformance harness in Task 7 against a real backend where available; the SQLite-flavored `MySqlSessionStore::sqlite` constructor is used by some tests — verify via Task 7. No isolated MySQL test here to avoid requiring a live MySQL instance.)

```bash
git add src/bcs/crates/services/bcs-session-store/src/mysql.rs
git commit -m "feat(bcs): mysql store collect/uncollect/list_collected_by_group"
```

---

## Task 6: Application service — real impls delegating to repo

**Files:**
- Modify: `src/bcs/crates/services/bcs-session/src/application.rs`

**Interfaces:**
- Consumes: `SessionRepoPort` new methods (Tasks 4–5).
- Produces: `SessionManagementService` real impls (used by HTTP in Task 8).

- [ ] **Step 1: Implement the three methods on `SessionManagementServiceImpl`**

In `application.rs`, inside the `impl SessionManagementService for ...` block (after `delete`):

```rust
    async fn collect(
        &self,
        session_id: &str,
        bot_uuid: &str,
    ) -> Result<(), SessionUseCaseError> {
        // Verify the session exists so collect can return a clean NotFound
        // rather than relying solely on the side-table UPDATE row count.
        if self.repo.get(session_id).await.is_none() {
            return Err(SessionUseCaseError::NotFound(session_id.to_string()));
        }
        self.repo.collect(session_id, bot_uuid).await?;
        Ok(())
    }

    async fn uncollect(
        &self,
        session_id: &str,
        bot_uuid: &str,
    ) -> Result<(), SessionUseCaseError> {
        if self.repo.get(session_id).await.is_none() {
            return Err(SessionUseCaseError::NotFound(session_id.to_string()));
        }
        // repo.uncollect is idempotent at the row level; surface nothing but
        // the session-existence check above.
        self.repo.uncollect(session_id, bot_uuid).await?;
        Ok(())
    }

    async fn list_collected_by_group(
        &self,
        group_id: &str,
        bot_uuid: &str,
        status: Option<SessionStatus>,
        title_contains: Option<&str>,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<Session>, SessionUseCaseError> {
        Ok(self
            .repo
            .list_collected_by_group(group_id, bot_uuid, status, title_contains, offset, limit)
            .await)
    }
```

Confirm `Session`, `SessionStatus` are in scope (they are — used by `list_by_group` above). `self.repo.get(...)` returns `Option<Session>` (verified port).

- [ ] **Step 2: Verify the noop impl**

Run: `cd src/bcs && grep -n "impl SessionManagementService" crates/services/bcs-session/src/noop.rs`

`noop.rs` is a direct `impl`. With default impls on the trait, noop does NOT need overrides for the new methods — but verify the compile:

Run: `cd src/bcs && cargo check -p bcs-session`
Expected: PASS (noop inherits defaults; that's fine — noop is `Ok(Vec::new())`-ish for list and `Err(Internal)` for collect/uncollect, which matches its no-op intent).

- [ ] **Step 3: Commit**

```bash
git add src/bcs/crates/services/bcs-session/src/application.rs
git commit -m "feat(bcs): session application service collect/uncollect/list_collected_by_group"
```

---

## Task 7: Repo contract harness — extend `session_repo_port_contract_tests`

Per the spec's §13.4, the new port contract methods must be exercised by the harness so both MySQL and Memory conform. The existing conformance entry `bcs-session-store/tests/conformance_session_repo.rs` calls `session_repo_port_contract_tests` and already exercises Memory; the same harness would cover a MySQL/SQLite backend if wired.

**Files:**
- Modify: `src/bcs/crates/test-support/bcs-test-support/src/contract/repo/mod.rs`

**Interfaces:**
- Consumes: `SessionRepoPort` new methods (Task 1), real impls (Tasks 4–5).
- Produces: extended contract assertions automatically run by `conformance_session_repo.rs`.

- [ ] **Step 1: Append collection assertions to the harness**

In `session_repo_contract_tests<T: SessionRepoPort + ?Sized>` (verified at `mod.rs:770`, after the `list_group_ids_by_session_participant` assertions near the end of the function body, before the closing `}`):

```rust
    // ── session collection (收藏) contract ───────────────────
    // Create a second participant so we can assert per-bot isolation.
    let collect_session: Session = repo
        .create(
            svc.group_id,
            NewSessionParams {
                session_kind: SessionKind::Chat,
                participants: vec![
                    Participant::bot("bot-collector", ParticipantRole::Driver),
                    Participant::bot("bot-other", ParticipantRole::Consultant),
                ],
                ..Default::default()
            },
        )
        .await
        .expect("create session for collection contract");

    // Not collected yet
    assert!(repo
        .list_collected_by_group(svc.group_id, "bot-collector", None, None, 0, 10)
        .await
        .is_empty());

    // collect by a participant
    repo.collect(&collect_session.id, "bot-collector")
        .await
        .expect("collect by participant");
    let collected = repo
        .list_collected_by_group(svc.group_id, "bot-collector", None, None, 0, 10)
        .await;
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].id, collect_session.id);

    // per-bot isolation: other participant sees nothing
    assert!(repo
        .list_collected_by_group(svc.group_id, "bot-other", None, None, 0, 10)
        .await
        .is_empty());

    // collect by non-participant errors
    let err = repo.collect(&collect_session.id, "bot-stranger").await;
    assert!(err.is_err(), "collect by non-participant must error");

    // repeat collect is idempotent (no error)
    repo.collect(&collect_session.id, "bot-collector")
        .await
        .expect("repeat collect idempotent");

    // uncollect removes it
    repo.uncollect(&collect_session.id, "bot-collector")
        .await
        .expect("uncollect");
    assert!(repo
        .list_collected_by_group(svc.group_id, "bot-collector", None, None, 0, 10)
        .await
        .is_empty());

    // uncollect of a never-collected / non-participant is idempotent Ok
    repo.uncollect(&collect_session.id, "bot-collector")
        .await
        .expect("uncollect not-collected idempotent");
    repo.uncollect(&collect_session.id, "bot-stranger")
        .await
        .expect("uncollect non-participant idempotent");
```

> The harness already built `svc` (a session var named `svc` of kind ServiceInvocation, verified in the existing body) and uses `svc.group_id`. `NewSessionParams`, `Participant`, `ParticipantRole`, `SessionKind`, `Session` are already imported in this contract file (verify with `grep -n "use " crates/test-support/bcs-test-support/src/contract/repo/mod.rs | head` and add any missing import before writing this block).

- [ ] **Step 2: Run the conformance test against Memory**

Run: `cd src/bcs && cargo test -p bcs-session-store --test conformance_session_repo -- --nocapture`
Expected: PASS (memory backend satisfies the new contract assertions).

- [ ] **Step 3: Commit**

```bash
git add src/bcs/crates/test-support/bcs-test-support/src/contract/repo/mod.rs
git commit -m "test(bcs): extend session repo contract harness with collection assertions"
```

---

## Task 8: HTTP — `collected` list filter + collect/uncollect handlers + routes

**Files:**
- Modify: `src/bcs/crates/adapters/http/bcs-http/src/routes/sessions.rs`
- Modify: `src/bcs/crates/adapters/http/bcs-http/src/router.rs`

**Interfaces:**
- Consumes: `SessionManagementService` new methods (Task 6), `resolve_group_chat_caller`, registry ownership check.
- Produces: `GET /groups/{id}/sessions?collected=true`, `POST /sessions/{sid}/collect`, `DELETE /sessions/{sid}/collect`.

- [ ] **Step 1: Add `collected` to the list query struct + branch**

In `routes/sessions.rs`, extend `ListSessionsQuery` (verified at line 522):

```rust
#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    #[serde(default)]
    pub status: Option<bcs_service_api::SessionStatus>,
    /// Optional search query: filters by session_title substring (case-insensitive).
    #[serde(default)]
    pub q: Option<String>,
    /// Filter sessions by participant (bot_uuid or human actor_id).
    #[serde(default)]
    pub participant: Option<String>,
    /// When true, return only sessions collected by `participant` (requires participant).
    #[serde(default)]
    pub collected: Option<bool>,
    #[serde(default)]
    pub offset: u64,
    #[serde(default = "default_limit")]
    pub limit: u64,
}
```

In `list_sessions_for_group`, right after the query is parsed and before the existing `state.services.session_management.list_by_group(...)` call, branch on `collected`:

```rust
    // collected filter: requires participant, returns only sessions the bot
    // has collected in this group. Does NOT run the legacy auto-create path.
    if params.collected == Some(true) {
        let bot_uuid = match params.participant.as_deref() {
            Some(p) => p,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid_params",
                        "message": "collected filter requires participant"
                    })),
                )
                    .into_response();
            }
        };
        let collected_sessions = match state
            .services
            .session_management
            .list_collected_by_group(
                &group_id,
                bot_uuid,
                params.status,
                params.q.as_deref(),
                params.offset,
                params.limit,
            )
            .await
        {
            Ok(v) => v,
            Err(e) => return session_error_to_response(&e),
        };
        let items: Vec<Value> = collected_sessions
            .iter()
            .map(|s| session_to_json(s))
            .collect();
        return Json(serde_json::json!({
            "items": items,
            "group_id": group_id,
        }))
        .into_response();
    }
```

Place this branch at the very top of the handler body (before the existing `list_by_group` call) so the legacy auto-create logic is skipped for the collected filter. `StatusCode`, `Json`, `Value` are already imported (verified at top of `sessions.rs`).

- [ ] **Step 2: Add `collect_session` and `uncollect_session` handlers**

Append at the end of `routes/sessions.rs` (before the `#[cfg(test)] mod tests` block):

```rust
// ---------------------------------------------------------------
// POST /sessions/{sid}/collect
// DELETE /sessions/{sid}/collect
//
// Mark / unmark a session as collected by a bot. Caller resolves via
// resolve_group_chat_caller (bot token -> that bot; human cookie -> must
// supply an owned bot via the `participant` body/query field, ownership
// checked via registry.list_bots_by_creator).
// ---------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct CollectSessionRequest {
    /// Target bot that performs the collect. Required for human callers;
    /// ignored (defaults to the calling bot) when a bot token is used.
    #[serde(default)]
    pub participant: Option<String>,
}

async fn resolve_collector_bot(
    state: &HttpAppState,
    headers: &HeaderMap,
    uri: &Uri,
    participant: Option<&str>,
) -> Result<String, Response> {
    let caller = match resolve_group_chat_caller(state, headers, uri).await {
        Ok(c) => c,
        Err(_) => {
            return Err(
                (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "unauthorized"})),
                )
                    .into_response(),
            )
        }
    };
    match caller {
        GroupChatCaller::Bot { bot_uuid } => Ok(bot_uuid),
        GroupChatCaller::Human(h) => {
            let bot_uuid = participant.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid_params",
                        "message": "human caller must supply participant"
                    })),
                )
                    .into_response()
            })?;
            let owns = state
                .services
                .registry
                .list_bots_by_creator(&h.staff_no)
                .await
                .iter()
                .any(|b| b.bot_uuid == bot_uuid);
            if !owns {
                return Err(
                    (
                        StatusCode::FORBIDDEN,
                        Json(serde_json::json!({
                            "error": "forbidden",
                            "message": format!("caller does not own bot {}", bot_uuid)
                        })),
                    )
                        .into_response(),
                );
            }
            Ok(bot_uuid.to_string())
        }
    }
}

pub async fn collect_session(
    State(state): State<HttpAppState>,
    Path(sid): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(body): Json<CollectSessionRequest>,
) -> impl IntoResponse {
    let collector = match resolve_collector_bot(&state, &headers, &uri, body.participant.as_deref()).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    match state.services.session_management.collect(&sid, &collector).await {
        Ok(()) => Json(serde_json::json!({"collected": true, "session_id": sid})).into_response(),
        Err(e) => session_error_to_response(&e),
    }
}

pub async fn uncollect_session(
    State(state): State<HttpAppState>,
    Path(sid): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Query(body): Query<CollectSessionRequest>,
) -> impl IntoResponse {
    let collector = match resolve_collector_bot(&state, &headers, &uri, body.participant.as_deref()).await {
        Ok(b) => b,
        Err(resp) => return resp,
    };
    match state.services.session_management.uncollect(&sid, &collector).await {
        Ok(()) => Json(serde_json::json!({"collected": false, "session_id": sid})).into_response(),
        Err(e) => session_error_to_response(&e),
    }
}
```

> `DELETE` cannot reliably carry a JSON body across all clients/proxies, so `uncollect_session` reads `participant` from the query string (`Query<CollectSessionRequest>`). `collect_session` reads it from the JSON body. Both paths go through the same `resolve_collector_bot` helper. For bot-token callers the `participant` field is unnecessary and ignored.

- [ ] **Step 3: Register the route**

In `router.rs`, after the `/sessions/{sid}/complete` route block (verified at line 253–256) and before `/sessions/{sid}/members`, add:

```rust
        .route(
            "/sessions/{sid}/collect",
            post(routes::sessions::collect_session)
                .delete(routes::sessions::uncollect_session),
        )
```

`post` is already imported in `router.rs` (used throughout). Verify `post` and `delete` are both in the axum import list; add `delete` if missing (grep: `cd src/bcs && grep -n "use axum::routing" crates/adapters/http/bcs-http/src/router.rs`).

- [ ] **Step 4: Verify it compiles**

Run: `cd src/bcs && cargo check -p bcs-http`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/bcs/crates/adapters/http/bcs-http/src/routes/sessions.rs src/bcs/crates/adapters/http/bcs-http/src/router.rs
git commit -m "feat(bcs): HTTP collected list filter + collect/uncollect endpoints"
```

---

## Task 9: HTTP contract tests

**Files:**
- Create: `src/bcs/crates/adapters/http/bcs-http/tests/session_collection_contract.rs`

**Interfaces:**
- Consumes: HTTP endpoints (Task 8), existing test-support mock `SessionManagementService` pattern.

- [ ] **Step 1: Mirror an existing contract test's harness setup**

First inspect the simplest existing session contract test to copy its app-state / mock-service wiring:

Run: `cd src/bcs && sed -n '1,80p' crates/adapters/http/bcs-http/tests/session_list_contract.rs`

This file shows how to build `HttpAppState` with a mock `SessionManagementService` and drive `axum` routes via `oneshot`. The mock there implements `SessionManagementService`; thanks to default impls it already compiles, but those defaults return errors/empty for collection. To test collection at the HTTP layer, **add concrete overrides** for the three new methods on that test's mock (or — recommended — create a focused mock local to the new test file).

- [ ] **Step 2: Write `session_collection_contract.rs`**

```rust
//! HTTP contract tests for session collection (collect/uncollect/collected list).

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use bcs_service_api::application::session::{
    CreateOrReactivateCommand, CreateOrReactivateOutcome, SessionManagementService,
    SessionUseCaseError,
};
use bcs_service_api::types::{Participant, Session, SessionStatus};

// A minimal mock that records collections in memory.
#[derive(Default)]
struct MockCollectionService {
    sessions: Arc<Mutex<Vec<Session>>>,
    collected: Arc<Mutex<HashSet<(String, String)>>>, // (session_id, bot_uuid)
}

#[async_trait]
impl SessionManagementService for MockCollectionService {
    async fn create_or_reactivate(
        &self,
        cmd: CreateOrReactivateCommand,
    ) -> Result<CreateOrReactivateOutcome, SessionUseCaseError> {
        let session = Session {
            id: cmd.session_id.unwrap_or_else(|| format!("{}:aaaaaaaa", cmd.group_id)),
            group_id: cmd.group_id,
            status: SessionStatus::Running,
            participants: cmd.params.participants,
            ..Default::default()
        };
        self.sessions.lock().await.push(session.clone());
        Ok(CreateOrReactivateOutcome { session, created: true })
    }
    async fn get(&self, session_id: &str) -> Result<Option<Session>, SessionUseCaseError> {
        Ok(self.sessions.lock().await.iter().find(|s| s.id == session_id).cloned())
    }
    async fn belongs_to_group(
        &self, _s: &str, _g: &str,
    ) -> Result<bool, SessionUseCaseError> { Ok(true) }
    async fn list_by_group(
        &self, group_id: &str, _status: Option<SessionStatus>, _o: u64, _l: u64,
        _q: Option<&str>, _p: Option<&str>,
    ) -> Result<Vec<Session>, SessionUseCaseError> {
        Ok(self.sessions.lock().await.iter().filter(|s| s.group_id == group_id).cloned().collect())
    }
    async fn count_running_service(&self, _g: &str) -> Result<u64, SessionUseCaseError> { Ok(0) }
    async fn list_running_service(&self, _o: u64, _l: u64) -> Result<Vec<Session>, SessionUseCaseError> { Ok(vec![]) }
    async fn update_callback_status(&self, _s: &str, _st: &str) -> Result<(), SessionUseCaseError> { Ok(()) }
    async fn complete_if_running(&self, _s: &str, _o: Option<serde_json::Value>, _e: Option<String>) -> Result<Option<Session>, SessionUseCaseError> { Ok(None) }
    async fn add_participant(&self, _s: &str, _p: Participant) -> Result<Session, SessionUseCaseError> { unimplemented!() }
    async fn remove_participant(&self, _s: &str, _b: &str) -> Result<Session, SessionUseCaseError> { unimplemented!() }
    async fn update_participant_mode(&self, _s: &str, _b: &str, _m: bcs_service_api::types::ParticipantMode) -> Result<Session, SessionUseCaseError> { unimplemented!() }
    async fn update_title(&self, _s: &str, _t: Option<String>) -> Result<Session, SessionUseCaseError> { unimplemented!() }
    async fn list_group_ids_by_session_participant(&self, _b: &str) -> Result<Vec<String>, SessionUseCaseError> { Ok(vec![]) }
    async fn delete(&self, _s: &str) -> Result<bool, SessionUseCaseError> { Ok(true) }

    // collection overrides
    async fn collect(&self, session_id: &str, bot_uuid: &str) -> Result<(), SessionUseCaseError> {
        let exists = self.sessions.lock().await.iter().any(|s| s.id == session_id);
        if !exists {
            return Err(SessionUseCaseError::NotFound(session_id.to_string()));
        }
        self.collected.lock().await.insert((session_id.to_string(), bot_uuid.to_string()));
        Ok(())
    }
    async fn uncollect(&self, session_id: &str, bot_uuid: &str) -> Result<(), SessionUseCaseError> {
        let exists = self.sessions.lock().await.iter().any(|s| s.id == session_id);
        if !exists {
            return Err(SessionUseCaseError::NotFound(session_id.to_string()));
        }
        self.collected.lock().await.remove(&(session_id.to_string(), bot_uuid.to_string()));
        Ok(())
    }
    async fn list_collected_by_group(
        &self, group_id: &str, bot_uuid: &str, _status: Option<SessionStatus>,
        _q: Option<&str>, _o: u64, _l: u64,
    ) -> Result<Vec<Session>, SessionUseCaseError> {
        let sessions = self.sessions.lock().await;
        let collected = self.collected.lock().await;
        let mut out: Vec<Session> = sessions
            .iter()
            .filter(|s| s.group_id == group_id && collected.contains(&(s.id.clone(), bot_uuid.to_string())))
            .cloned()
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        Ok(out)
    }
}
```

> IMPORTANT: This mock is illustrative. The existing HTTP contract tests (e.g. `session_list_contract.rs`) wire `HttpAppState` via a shared builder. Before finalizing, copy THAT file's `build_app()` / state-construction helpers verbatim and substitute `MockCollectionService`. Reuse the same `axum::Router` + `tower::ServiceExt::oneshot` request pattern. Do NOT invent a new state-builder signature.

- [ ] **Step 3: Write the assertions**

```rust
// After building `app` with MockCollectionService and a bot-token auth setup
// mirroring session_list_contract.rs:

// 1. collected=true without participant -> 400
let resp = app
    .clone()
    .oneshot(Request::builder().method("GET").uri("/groups/g1/sessions?collected=true").body(Body::empty()).unwrap())
    .await.unwrap();
assert_eq!(resp.status(), 400);

// 2. collect (bot token) -> 200, then list shows it
//    (drive create session first, then POST /sessions/{sid}/collect with
//     the bot's bearer token; then GET /groups/g1/sessions?participant=bot1&collected=true
//     returns the session; assert items.len() == 1.)

// 3. DELETE /sessions/{sid}/collect -> 200, list empty.

// 4. collect on a non-existent session -> 404.

// 5. human caller without participant -> 400; human with non-owned bot -> 403.
```

Fill the request bodies, bearer-token headers, and exact extractors by copying from `session_list_contract.rs`'s existing tests — the auth helper and `Body`/`Request` imports are already established there. Each test function ends with the status assertion shown.

- [ ] **Step 4: Run the tests**

Run: `cd src/bcs && cargo test -p bcs-http --test session_collection_contract -- --nocapture`
Expected: PASS (all assertions green).

- [ ] **Step 5: Commit**

```bash
git add src/bcs/crates/adapters/http/bcs-http/tests/session_collection_contract.rs
git commit -m "test(bcs): HTTP contract tests for session collection"
```

---

## Task 10: Workspace check + existing tests no-regression

- [ ] **Step 1: Build the whole workspace**

Run: `cd src/bcs && cargo build --workspace`
Expected: PASS.

- [ ] **Step 2: Run the session-list contract + repo conformance suites**

Run: `cd src/bcs && cargo test -p bcs-http --test session_list_contract && cargo test -p bcs-session-store --test conformance_session_repo`
Expected: PASS (proves `collected` absence keeps the old list path unchanged, and new contract holds).

- [ ] **Step 3: Run the full workspace test suite**

Run: `cd src/bcs && cargo test --workspace`
Expected: PASS. If a pre-existing unrelated test fails, note it but do not attribute to this change; if a test fails because of a new default-impl override an existing mock relies on, add the override (rare; default impls are designed to avoid this).

- [ ] **Step 4: Commit if any test-supporting tweaks were made**

(Usually none. If `cargo check` revealed a mock that needed an override, commit it here.)

```bash
# only if files changed
git add -A && git commit -m "test(bcs): finalize session collection test wiring"
```

---

## Self-Review

**1. Spec coverage**
- §3 decisions → Task 1 (default-impl B-ii), Task 8 (collected query + /collect routes), Task 8 Step 2 (collector resolution: bot token vs human+owned participant). ✓
- §4.1 column + index → Task 2 (MySQL), Task 3 (SQLite). tinyint(4) / INTEGER. ✓
- §4.2 domain untouched → no task touches `Participant`; verified in Task 1/4/5. ✓
- §4.3 boundary behavior (leave drops flag) → Task 4 memory pruning in remove/delete; MySQL relies on existing `DELETE FROM bcs_session_participants` in `remove_participant` (verified, no change needed) and `add_participant` ON CONFLICT DO NOTHING (verified). ✓
- §5 layering → Task 1 (port/service), Task 6 (app delegation), Tasks 4–5 (stores), Task 8 (HTTP). No core layer touched. ✓
- §6.1 list filter + 400 → Task 8 Step 1. ✓
- §6.2 collect + 404 non-participant + 401 + ownership → Task 8 Step 2. ✓
- §6.3 uncollect idempotent / only session-not-found 404 → Task 8 + Task 6 (app pre-check) + Task 5/4 (repo idempotent). ✓
- §7 auth via `resolve_group_chat_caller` + `list_bots_by_creator` → Task 8 Step 2. ✓
- §8 default impls + tradeoff note → Task 1 (code comment) + Task 7 harness (conformance backstop). ✓
- §9.1 SQL + 9.2 index-aligned JOIN → Task 5 (includes `sp.group_id = ?`). ✓
- §10 memory set → Task 4. ✓
- §11 route registration → Task 8 Step 3. ✓
- §12 error table → covered across Tasks 6/8. ✓
- §13.1 memory unit tests → Task 4. ✓
- §13.2 HTTP contract → Task 9. ✓
- §13.3 no-regression → Task 10. ✓
- §13.4 repo conformance → Task 7. ✓
- §14 migrations MySQL+SQLite existing-DB → Tasks 2–3 (ensure function). ✓

**2. Placeholder scan:** Task 9 Step 2 contains an explicit illustrative mock with a note to mirror the existing file's state-builder. This is the one place requiring judgment at execution; flagged clearly with the exact source file to copy from and the exact adaptation needed. No "TBD"/"handle edge cases" phrasings. Acceptable.

**3. Type consistency:** `collect(session_id, bot_uuid) -> ServiceResult<()>` (repo) maps to `Result<(), SessionUseCaseError>` (service) and HTTP returns `{"collected": bool}`. `list_collected_by_group(group_id, bot_uuid, status, title_contains, offset, limit)` signature is identical across port/service/HTTP-call site/MySQL/Memory/harness. `CollectSessionRequest.participant: Option<String>` used by both handlers. `idx_collected` name consistent in Tasks 2 and 3. ✓

No gaps found. Plan is complete.