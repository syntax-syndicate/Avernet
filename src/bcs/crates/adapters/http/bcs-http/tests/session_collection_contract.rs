//! HTTP contract tests for session collection (collect/uncollect/collected list).
//!
//! Exercises the three endpoints added in Task 8:
//!   GET  /groups/{id}/sessions?collected=true&participant=<bot>
//!   POST /sessions/{sid}/collect
//!   DELETE /sessions/{sid}/collect
//!
//! Mirrors the app-state wiring from `session_list_contract.rs` with a
//! self-contained mock `CollectionMock` that records collections in memory.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use bcs_auth_api::{AuthError, UserIdentityInfo};
use bcs_bot::BotCore;
use bcs_group::GroupStore;
use bcs_http::{
    router::build_router,
    state::{HttpAppState, HttpUserIdentity, UserIdentityPort},
};
use bcs_service_api::{
    ActorKind, BotCapabilities, BotRegistryCoreService, CreateOrReactivateCommand,
    CreateOrReactivateOutcome, Group, GroupCoreService, Participant, ParticipantMode,
    ParticipantRole, Session, SessionKind, SessionManagementService, SessionStatus,
    SessionUseCaseError,
};
use bcs_services_container::Services;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::Mutex;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// mock service
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CollectionMock {
    sessions: Mutex<Vec<Session>>,
    collected: Mutex<HashSet<(String, String)>>, // (session_id, bot_uuid)
}

#[async_trait]
impl SessionManagementService for CollectionMock {
    async fn create_or_reactivate(
        &self,
        cmd: CreateOrReactivateCommand,
    ) -> Result<CreateOrReactivateOutcome, SessionUseCaseError> {
        let id = cmd
            .params
            .id
            .clone()
            .unwrap_or_else(|| format!("{}:abcdef12", cmd.group_id));
        let session = Session {
            id: id.clone(),
            group_id: cmd.group_id.clone(),
            session_title: cmd.params.session_title.clone(),
            env: None,
            status: SessionStatus::Running,
            session_kind: cmd.params.session_kind,
            participants: cmd.params.participants.clone(),
            group_version: cmd.params.group_version,
            caller_id: cmd.params.caller_id.clone(),
            input: cmd.params.input.clone(),
            output: None,
            error_message: None,
            callback_status: None,
            activation_count: 1,
            caller_principal: cmd.params.caller_principal.clone(),
            created_by: cmd.params.created_by.clone(),
            created_at: 1,
            updated_at: 1,
            completed_at: None,
            meta: cmd.params.meta.clone(),
            current_msg_seq: 0,
            participant_join_seq: None,
        };
        self.sessions.lock().await.push(session.clone());
        Ok(CreateOrReactivateOutcome {
            session,
            created: true,
        })
    }

    async fn get(&self, sid: &str) -> Result<Option<Session>, SessionUseCaseError> {
        Ok(self
            .sessions
            .lock()
            .await
            .iter()
            .find(|s| s.id == sid)
            .cloned())
    }

    async fn belongs_to_group(
        &self,
        session_id: &str,
        group_id: &str,
    ) -> Result<bool, SessionUseCaseError> {
        Ok(self
            .sessions
            .lock()
            .await
            .iter()
            .any(|s| s.id == session_id && s.group_id == group_id))
    }

    async fn list_by_group(
        &self,
        group_id: &str,
        _status: Option<SessionStatus>,
        _offset: u64,
        _limit: u64,
        _title_contains: Option<&str>,
        _participant_id: Option<&str>,
    ) -> Result<Vec<Session>, SessionUseCaseError> {
        Ok(self
            .sessions
            .lock()
            .await
            .iter()
            .filter(|s| s.group_id == group_id)
            .cloned()
            .collect())
    }

    async fn count_running_service(&self, _group_id: &str) -> Result<u64, SessionUseCaseError> {
        Ok(0)
    }

    async fn list_running_service(
        &self,
        _offset: u64,
        _limit: u64,
    ) -> Result<Vec<Session>, SessionUseCaseError> {
        Ok(Vec::new())
    }

    async fn update_callback_status(
        &self,
        _session_id: &str,
        _status: &str,
    ) -> Result<(), SessionUseCaseError> {
        Ok(())
    }

    async fn complete_if_running(
        &self,
        _session_id: &str,
        _output: Option<Value>,
        _error: Option<String>,
    ) -> Result<Option<Session>, SessionUseCaseError> {
        Ok(None)
    }

    async fn add_participant(
        &self,
        _session_id: &str,
        _participant: Participant,
    ) -> Result<Session, SessionUseCaseError> {
        unimplemented!("not used by collection tests")
    }

    async fn remove_participant(
        &self,
        _session_id: &str,
        _bot_uuid: &str,
    ) -> Result<Session, SessionUseCaseError> {
        unimplemented!("not used by collection tests")
    }

    async fn update_participant_mode(
        &self,
        _session_id: &str,
        _bot_uuid: &str,
        _mode: ParticipantMode,
    ) -> Result<Session, SessionUseCaseError> {
        unimplemented!("not used by collection tests")
    }

    async fn update_title(
        &self,
        _session_id: &str,
        _title: Option<String>,
    ) -> Result<Session, SessionUseCaseError> {
        unimplemented!("not used by collection tests")
    }

    async fn list_group_ids_by_session_participant(
        &self,
        _bot_uuid: &str,
    ) -> Result<Vec<String>, SessionUseCaseError> {
        Ok(Vec::new())
    }

    async fn delete(&self, _session_id: &str) -> Result<bool, SessionUseCaseError> {
        Ok(false)
    }

    // ── collection overrides ──────────────────────────────────────────

    async fn collect(
        &self,
        session_id: &str,
        bot_uuid: &str,
    ) -> Result<(), SessionUseCaseError> {
        let exists = self
            .sessions
            .lock()
            .await
            .iter()
            .any(|s| s.id == session_id);
        if !exists {
            return Err(SessionUseCaseError::NotFound(session_id.to_string()));
        }
        self.collected
            .lock()
            .await
            .insert((session_id.to_string(), bot_uuid.to_string()));
        Ok(())
    }

    async fn uncollect(
        &self,
        session_id: &str,
        bot_uuid: &str,
    ) -> Result<(), SessionUseCaseError> {
        let exists = self
            .sessions
            .lock()
            .await
            .iter()
            .any(|s| s.id == session_id);
        if !exists {
            return Err(SessionUseCaseError::NotFound(session_id.to_string()));
        }
        self.collected
            .lock()
            .await
            .remove(&(session_id.to_string(), bot_uuid.to_string()));
        Ok(())
    }

    async fn list_collected_by_group(
        &self,
        group_id: &str,
        bot_uuid: &str,
        _status: Option<SessionStatus>,
        _title_contains: Option<&str>,
        _offset: u64,
        _limit: u64,
    ) -> Result<Vec<Session>, SessionUseCaseError> {
        let sessions = self.sessions.lock().await;
        let collected = self.collected.lock().await;
        let mut out: Vec<Session> = sessions
            .iter()
            .filter(|s| {
                s.group_id == group_id
                    && collected.contains(&(s.id.clone(), bot_uuid.to_string()))
            })
            .cloned()
            .collect();
        out.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

/// 1. `GET ...?collected=true` without participant → 400
#[tokio::test]
async fn collected_true_without_participant_returns_400() {
    let (app, _mock, _temp_dir, _registry) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/groups/group-1/sessions?collected=true")
                .header("authorization", "Bearer driver-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["error"], "invalid_params");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("participant"),
        "expected message about missing participant, got: {}",
        body
    );
}

/// 2. Collect (bot token) → 200 `{collected:true}`; collected list returns
///    the session; repeat collect is idempotent.
#[tokio::test]
async fn collect_and_list_collected_via_bot_token() {
    let (app, mock, _temp_dir, _registry) = test_app().await;

    // Pre-seed a session.
    let sid = "group-1:aaaaaaaa";
    mock.sessions.lock().await.push(Session {
        id: sid.to_string(),
        group_id: "group-1".to_string(),
        session_title: Some("Test session".to_string()),
        env: None,
        status: SessionStatus::Running,
        session_kind: SessionKind::Chat,
        participants: vec![Participant::bot("driver-bot", ParticipantRole::Driver)],
        group_version: Some(1),
        caller_id: None,
        input: None,
        output: None,
        error_message: None,
        callback_status: None,
        activation_count: 1,
        caller_principal: None,
        created_by: None,
        created_at: 1,
        updated_at: 1,
        completed_at: None,
        meta: None,
        current_msg_seq: 0,
        participant_join_seq: None,
    });

    // POST /sessions/{sid}/collect with bot token
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/sessions/{sid}/collect"))
                .header("content-type", "application/json")
                .header("authorization", "Bearer driver-token")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["collected"], true);
    assert_eq!(body["session_id"], sid);

    // GET collected list — must now contain the session.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/groups/group-1/sessions?participant=driver-bot&collected=true"
                ))
                .header("authorization", "Bearer driver-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "collected list must contain the session");
    assert_eq!(items[0]["session_id"], sid);

    // Repeat collect — idempotent (still 200).
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/sessions/{sid}/collect"))
                .header("content-type", "application/json")
                .header("authorization", "Bearer driver-token")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["collected"], true);
}

/// 3. DELETE /sessions/{sid}/collect → 200 `{collected:false}`;
///    collected list is empty afterwards.
#[tokio::test]
async fn uncollect_removes_from_collected_list() {
    let (app, mock, _temp_dir, _registry) = test_app().await;

    let sid = "group-1:bbbbbbbb";
    mock.sessions.lock().await.push(Session {
        id: sid.to_string(),
        group_id: "group-1".to_string(),
        session_title: Some("To be uncollected".to_string()),
        env: None,
        status: SessionStatus::Running,
        session_kind: SessionKind::Chat,
        participants: vec![Participant::bot("driver-bot", ParticipantRole::Driver)],
        group_version: Some(1),
        caller_id: None,
        input: None,
        output: None,
        error_message: None,
        callback_status: None,
        activation_count: 1,
        caller_principal: None,
        created_by: None,
        created_at: 1,
        updated_at: 1,
        completed_at: None,
        meta: None,
        current_msg_seq: 0,
        participant_join_seq: None,
    });

    // Collect first.
    mock.collected
        .lock()
        .await
        .insert((sid.to_string(), "driver-bot".to_string()));

    // DELETE uncollect with bot token (participant via query param for bot is
    // optional, so we omit it — the bot token resolves the collector).
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/sessions/{sid}/collect"))
                .header("authorization", "Bearer driver-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["collected"], false);
    assert_eq!(body["session_id"], sid);

    // GET collected list — must be empty now.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/groups/group-1/sessions?participant=driver-bot&collected=true"
                ))
                .header("authorization", "Bearer driver-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    let items = body["items"].as_array().expect("items array");
    assert!(
        items.is_empty(),
        "collected list must be empty after uncollect"
    );
}

/// 4. Collect on a non-existent session → 404.
#[tokio::test]
async fn collect_nonexistent_session_returns_404() {
    let (app, _mock, _temp_dir, _registry) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/sessions/nonexistent-sid/collect")
                .header("content-type", "application/json")
                .header("authorization", "Bearer driver-token")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap_or("")
            .contains("nonexistent"),
        "expected error mentioning the session id, got: {}",
        body
    );
}

/// 5. Human caller without participant → 400; human with a non-owned bot → 403.
#[tokio::test]
async fn human_caller_collect_enforces_participant_and_ownership() {
    let (app, mock, _temp_dir, _registry) = human_app("alice", &["owned-bot"]).await;

    let sid = "group-1:cccccccc";
    mock.sessions.lock().await.push(Session {
        id: sid.to_string(),
        group_id: "group-1".to_string(),
        session_title: Some("Human test session".to_string()),
        env: None,
        status: SessionStatus::Running,
        session_kind: SessionKind::Chat,
        participants: vec![Participant::bot("owned-bot", ParticipantRole::Driver)],
        group_version: Some(1),
        caller_id: None,
        input: None,
        output: None,
        error_message: None,
        callback_status: None,
        activation_count: 1,
        caller_principal: None,
        created_by: None,
        created_at: 1,
        updated_at: 1,
        completed_at: None,
        meta: None,
        current_msg_seq: 0,
        participant_join_seq: None,
    });

    // Human without participant → 400.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/sessions/{sid}/collect"))
                .header("content-type", "application/json")
                .body(Body::from(json!({}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "human without participant must get 400"
    );
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["error"], "invalid_params");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("participant"),
        "expected message about missing participant"
    );

    // Human with a non-owned bot → 403.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/sessions/{sid}/collect"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"participant": "unauthorized-bot"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "human with non-owned bot must get 403"
    );
    let body: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(body["error"], "forbidden");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("does not own"),
        "expected ownership error message, got: {}",
        body
    );
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

async fn test_app() -> (
    axum::Router,
    Arc<CollectionMock>,
    TempDir,
    Arc<BotCore>,
) {
    let temp_dir = TempDir::new().unwrap();
    let registry = Arc::new(BotCore::with_base_dir(temp_dir.path().to_path_buf()));
    register_bot(&registry, "driver-bot", "Driver").await;
    registry
        .store_token_mapping("driver-token".to_string(), "driver-bot".to_string())
        .await;

    let group_store = Arc::new(GroupStore::new());
    group_store
        .upsert(Group::new(
            "group-1",
            "driver-bot",
            vec![Participant {
                bot_uuid: "driver-bot".to_string(),
                bot_name: Some("Driver".to_string()),
                kind: None,
                role: ParticipantRole::Driver,
                actor_kind: ActorKind::Bot,
                mode: Some(ParticipantMode::default_for(ActorKind::Bot)),
            }],
        ))
        .await
        .unwrap();

    let sessions = Arc::new(CollectionMock::default());

    let mut services = Services::noop();
    services.registry = registry.clone();
    services.group = group_store;
    services.session_management = sessions.clone();

    let app = build_router(HttpAppState::new(services));
    (app, sessions, temp_dir, registry)
}

async fn human_app(
    staff_no: &str,
    owned_bot_ids: &[&str],
) -> (
    axum::Router,
    Arc<CollectionMock>,
    TempDir,
    Arc<BotCore>,
) {
    let temp_dir = TempDir::new().unwrap();
    let registry = Arc::new(BotCore::with_base_dir(temp_dir.path().to_path_buf()));

    // Register the human's owned bots and set created_by.
    for bot_id in owned_bot_ids {
        register_bot(&registry, bot_id, bot_id).await;
        registry
            .save_created_by(bot_id, staff_no, true)
            .await
            .unwrap();
    }

    // Register an unauthorized bot (not owned by this human).
    register_bot(&registry, "unauthorized-bot", "Unauthorized").await;
    registry
        .save_created_by("unauthorized-bot", "someone-else", true)
        .await
        .unwrap();

    let group_store = Arc::new(GroupStore::new());
    let driver = owned_bot_ids.first().copied().unwrap_or("owned-bot");
    let participants: Vec<Participant> = owned_bot_ids
        .iter()
        .map(|bid| Participant {
            bot_uuid: bid.to_string(),
            bot_name: Some(bid.to_string()),
            kind: None,
            role: if *bid == driver {
                ParticipantRole::Driver
            } else {
                ParticipantRole::Consultant
            },
            actor_kind: ActorKind::Bot,
            mode: Some(ParticipantMode::default_for(ActorKind::Bot)),
        })
        .collect();
    group_store
        .upsert(Group::new("group-1", driver, participants))
        .await
        .unwrap();

    let sessions = Arc::new(CollectionMock::default());

    let mut services = Services::noop();
    services.registry = registry.clone();
    services.group = group_store;
    services.session_management = sessions.clone();

    let identity_port: Arc<dyn UserIdentityPort + Send + Sync> = Arc::new(StaticHumanIdentity {
        staff_no: Some(staff_no.to_string()),
    });
    let app = build_router(HttpAppState::new(services).with_user_identity(identity_port));
    (app, sessions, temp_dir, registry)
}

async fn register_bot(registry: &BotCore, bot_id: &str, name: &str) {
    registry
        .register(
            bot_id.to_string(),
            BotCapabilities {
                name: Some(name.to_string()),
                summary: Some(format!("{name} summary")),
                visibility: "public".to_string(),
                ..BotCapabilities::default()
            },
        )
        .await
        .unwrap();
}

#[derive(Default)]
struct StaticHumanIdentity {
    staff_no: Option<String>,
}

#[async_trait]
impl UserIdentityPort for StaticHumanIdentity {
    async fn extract(
        &self,
        _headers: &HeaderMap,
        _uri: &axum::http::Uri,
    ) -> Option<HttpUserIdentity> {
        self.staff_no.as_ref().map(|sn| HttpUserIdentity {
            staff_no: Some(sn.clone()),
            nick_name: Some("Test".to_string()),
        })
    }

    async fn ensure_identity(
        &self,
        _auth_source: &str,
        _external_user_id: &str,
        _external_user_name: Option<&str>,
        _avatar: Option<&str>,
        _env: &str,
    ) -> Result<String, AuthError> {
        Ok("noop-identity".to_string())
    }

    async fn get_identity_by_token(
        &self,
        _token: &str,
    ) -> Result<Option<UserIdentityInfo>, AuthError> {
        Ok(None)
    }

    async fn get_identity_by_user_id(
        &self,
        _user_id: &str,
    ) -> Result<Option<UserIdentityInfo>, AuthError> {
        Ok(None)
    }

    async fn update_token(
        &self,
        _user_id: &str,
        _token: &str,
        _expire_at: u64,
    ) -> Result<(), AuthError> {
        Ok(())
    }
}