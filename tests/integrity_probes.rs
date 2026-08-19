//! Integrity probes for the decision engine — service level (the exact shape a composing
//! host uses) plus route level behind the REAL `company_auth` middleware with minted HS256
//! tokens (the production mounting).
//!
//! What the suite pins:
//! - the no-policy posture (pre-approved, zero steps — a tenant that never opts into
//!   policies behaves exactly as it would without approvals wired);
//! - policy filing materializes every step once, delegation pre-resolved;
//! - all_of quorum (one row per member, every row must approve), reject fails fast and
//!   skips siblings, sequential steps gate each other;
//! - decide-time authorization is engine-side (assigned / live delegation window),
//!   with delegation stamping `delegated_from` (the authority source, never the
//!   decider);
//! - idempotent filing including the concurrent loser (23505 → same live request);
//! - withdraw frees the per-resource unique for a fresh chain;
//! - dynamic approver kinds fail closed without a resolver (422);
//! - cross-tenant ids are 404s both directions, never leakage;
//! - the RLS fence under `SET ROLE` to a plain non-superuser (the serpa_app posture):
//!   unbound sees zero rows, bound sees exactly its company's rows.
//!
//! DB: DATABASE_URL wins, else the module's local test DB (`backbone_approvals_test` on
//! the metaphora dev postgres, migrated). Fresh random company ids per test so parallel
//! runs never collide. The suite connects as the DB owner (a superuser, whom RLS can
//! never bind) — every verb carries its company predicate in SQL (belt-and-braces), and
//! the fence itself is pinned under SET ROLE.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::from_fn_with_state;
use sqlx::{Acquire, Executor, PgPool};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

use backbone_approvals::application::service::{
    ApprovalsError, ApprovalsWriteService, ApproverActor, ApproverResolver, Decision, FileFiling,
};
use backbone_approvals::{
    create_guarded_approvals_routes, create_guarded_approvals_routes_with, ApprovalPriority,
    ApprovalResourceType, ApprovalStatus, ApprovalsModule,
};
use backbone_auth::company::{company_auth, CompanyVerifier};

const SECRET: &[u8] = b"approvals-integrity-probe-secret";

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://serpa:serpa_dev_password@127.0.0.1:5432/backbone_approvals_test".into()
    });
    PgPool::connect(&url).await.unwrap()
}

fn engine(pool: &PgPool) -> ApprovalsWriteService {
    ApprovalsWriteService::new(pool.clone())
}

async fn module(pool: &PgPool) -> ApprovalsModule {
    ApprovalsModule::builder()
        .with_database(pool.clone())
        .build()
        .unwrap()
}

fn token_for(company: Uuid, sub: Uuid) -> String {
    let exp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize
        + 3600;
    let claims = serde_json::json!({"sub": sub.to_string(), "company_id": company, "exp": exp});
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(SECRET),
    )
    .unwrap()
}

async fn req(
    app: axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    body: String,
) -> (StatusCode, String) {
    let app = app.route_layer(from_fn_with_state(
        CompanyVerifier::hs256(SECRET),
        company_auth,
    ));
    let r = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(r).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

// ─── seed helpers (raw SQL, owner connection) ─────────────────────────────────

async fn seed_policy(pool: &PgPool, company: Uuid, resource_type: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO approvals.approval_policies
               (id, company_id, resource_type, name, is_active, description, metadata)
           VALUES ($1, $2, $3::approval_resource_type, $4, true, NULL, '{}'::jsonb)"#,
    )
    .bind(id)
    .bind(company)
    .bind(resource_type)
    .bind("probe policy")
    .execute(pool)
    .await
    .unwrap();
    id
}

#[allow(clippy::too_many_arguments)]
async fn seed_template(
    pool: &PgPool,
    company: Uuid,
    policy: Uuid,
    step_no: i32,
    approver_kind: &str,
    approver_ref: Option<Uuid>,
    all_of: Option<serde_json::Value>,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO approvals.approval_step_templates
               (id, company_id, policy_id, step_no, approver_kind, approver_ref, sla_hours,
                all_of, metadata)
           VALUES ($1, $2, $3, $4, $5::approver_kind, $6, NULL, $7, '{}'::jsonb)"#,
    )
    .bind(id)
    .bind(company)
    .bind(policy)
    .bind(step_no)
    .bind(approver_kind)
    .bind(approver_ref)
    .bind(all_of)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn seed_delegation(
    pool: &PgPool,
    company: Uuid,
    approver: Uuid,
    delegate: Uuid,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) {
    sqlx::query(
        r#"INSERT INTO approvals.delegations
               (id, company_id, approver_id, delegate_to_id, valid_from, valid_to, status, metadata)
           VALUES ($1, $2, $3, $4, $5, $6, 'active', '{}'::jsonb)"#,
    )
    .bind(Uuid::new_v4())
    .bind(company)
    .bind(approver)
    .bind(delegate)
    .bind(from)
    .bind(to)
    .execute(pool)
    .await
    .unwrap();
}

fn filing(company: Uuid, resource: Uuid, requester: Uuid) -> FileFiling {
    FileFiling {
        company_id: company,
        resource_type: ApprovalResourceType::Leave,
        resource_id: resource,
        requested_by: requester,
        priority: ApprovalPriority::Normal,
        summary: serde_json::json!({"note": "probe filing"}),
    }
}

fn decide_as(company: Uuid, request: Uuid, step_no: i32, actor: Uuid, approve: bool) -> Decision {
    Decision {
        company_id: company,
        request_id: request,
        step_no,
        actor: ApproverActor { employee_id: actor },
        approve,
        comment: None,
    }
}

/// Scoped read of one step row for assertions.
async fn step_row(
    pool: &PgPool,
    company: Uuid,
    request: Uuid,
    assigned_to: Uuid,
) -> (String, Option<Uuid>) {
    let row: (String, Option<Uuid>) = sqlx::query_as(
        r#"SELECT status::text, delegated_from FROM approvals.approval_steps
            WHERE company_id = $1 AND request_id = $2 AND assigned_to = $3
              AND (metadata->>'deleted_at') IS NULL"#,
    )
    .bind(company)
    .bind(request)
    .bind(assigned_to)
    .fetch_one(pool)
    .await
    .unwrap();
    row
}

async fn count_steps(pool: &PgPool, company: Uuid, request: Uuid) -> i64 {
    sqlx::query_scalar(
        r#"SELECT count(*) FROM approvals.approval_steps
            WHERE company_id = $1 AND request_id = $2
              AND (metadata->>'deleted_at') IS NULL"#,
    )
    .bind(company)
    .bind(request)
    .fetch_one(pool)
    .await
    .unwrap()
}

// ─── 1: the no-policy posture ────────────────────────────────────────────────

#[tokio::test]
async fn no_policy_files_pre_approved_with_zero_steps() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let resource = Uuid::new_v4();
    let svc = engine(&pool);

    // No policy seeded for this company/resource type.
    let outcome = svc.file(filing(company, resource, employee)).await.unwrap();
    assert_eq!(outcome.verdict, ApprovalStatus::Approved);
    assert!(!outcome.already_filed);
    assert_eq!(count_steps(&pool, company, outcome.request_id).await, 0);

    // decided_at is stamped: the verdict is already terminal, not "awaiting something".
    let decided: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar(r#"SELECT decided_at FROM approvals.approval_requests WHERE id = $1"#)
            .bind(outcome.request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        decided.is_some(),
        "no-policy filings are decided at file time"
    );

    // status agrees, and a re-file of the same resource converges on the same request.
    assert_eq!(
        svc.status(company, outcome.request_id).await.unwrap(),
        ApprovalStatus::Approved
    );
    let again = svc.file(filing(company, resource, employee)).await.unwrap();
    assert!(again.already_filed);
    assert_eq!(again.request_id, outcome.request_id);
}

// ─── 2: a policy materializes the whole chain at file time ───────────────────

#[tokio::test]
async fn policy_materializes_every_step_once() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let approver1 = Uuid::new_v4();
    let approver2 = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        Some(approver1),
        None,
    )
    .await;
    seed_template(
        &pool,
        company,
        policy,
        2,
        "specific_employee",
        Some(approver2),
        None,
    )
    .await;
    let svc = engine(&pool);

    let resource = Uuid::new_v4();
    let outcome = svc.file(filing(company, resource, employee)).await.unwrap();
    assert_eq!(outcome.verdict, ApprovalStatus::Pending);
    assert_eq!(count_steps(&pool, company, outcome.request_id).await, 2);

    let request = svc.get_request(company, outcome.request_id).await.unwrap();
    assert_eq!(request.current_step, Some(1));

    // Step 2 exists from filing but the chain has not reached it.
    assert_eq!(
        step_row(&pool, company, outcome.request_id, approver2)
            .await
            .0,
        "pending"
    );
}

// ─── 3: all_of quorum ────────────────────────────────────────────────────────

#[tokio::test]
async fn all_of_quorum_requires_every_member() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let c = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        None,
        Some(serde_json::json!([a, b, c])),
    )
    .await;
    let svc = engine(&pool);

    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    // One row per member, all at step 1.
    assert_eq!(count_steps(&pool, company, outcome.request_id).await, 3);

    svc.decide(decide_as(company, outcome.request_id, 1, a, true))
        .await
        .unwrap();
    assert_eq!(
        svc.status(company, outcome.request_id).await.unwrap(),
        ApprovalStatus::Pending,
        "one of three approvals does not complete the step"
    );
    assert_eq!(
        step_row(&pool, company, outcome.request_id, b).await.0,
        "pending"
    );

    svc.decide(decide_as(company, outcome.request_id, 1, b, true))
        .await
        .unwrap();
    assert_eq!(
        svc.status(company, outcome.request_id).await.unwrap(),
        ApprovalStatus::Pending,
        "two of three approvals do not complete the step"
    );

    svc.decide(decide_as(company, outcome.request_id, 1, c, true))
        .await
        .unwrap();
    assert_eq!(
        svc.status(company, outcome.request_id).await.unwrap(),
        ApprovalStatus::Approved,
        "the last member's approval finishes the request"
    );
    assert_eq!(
        step_row(&pool, company, outcome.request_id, a).await.0,
        "approved"
    );
    assert_eq!(
        step_row(&pool, company, outcome.request_id, b).await.0,
        "approved"
    );
}

// ─── 4: reject fails fast ────────────────────────────────────────────────────

#[tokio::test]
async fn reject_fails_fast_and_skips_siblings() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let step2 = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        None,
        Some(serde_json::json!([a, b])),
    )
    .await;
    seed_template(
        &pool,
        company,
        policy,
        2,
        "specific_employee",
        Some(step2),
        None,
    )
    .await;
    let svc = engine(&pool);

    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();

    let verdict = svc
        .decide(decide_as(company, outcome.request_id, 1, a, false))
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Rejected);

    // The quorum sibling and the never-reached step 2 are both skipped — nothing lingers.
    assert_eq!(
        step_row(&pool, company, outcome.request_id, a).await.0,
        "rejected"
    );
    assert_eq!(
        step_row(&pool, company, outcome.request_id, b).await.0,
        "skipped"
    );
    assert_eq!(
        step_row(&pool, company, outcome.request_id, step2).await.0,
        "skipped"
    );

    // A late decide on the corpse is a 409, not a state change.
    let err = svc
        .decide(decide_as(company, outcome.request_id, 1, b, true))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::NotPending));
}

// ─── 5: sequencing ───────────────────────────────────────────────────────────

#[tokio::test]
async fn step_two_waits_for_step_one() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let approver1 = Uuid::new_v4();
    let approver2 = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        Some(approver1),
        None,
    )
    .await;
    seed_template(
        &pool,
        company,
        policy,
        2,
        "specific_employee",
        Some(approver2),
        None,
    )
    .await;
    let svc = engine(&pool);

    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();

    // Step 2's approver jumping the queue: 409 out_of_turn.
    let err = svc
        .decide(decide_as(company, outcome.request_id, 2, approver2, true))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::OutOfTurn));

    svc.decide(decide_as(company, outcome.request_id, 1, approver1, true))
        .await
        .unwrap();
    let request = svc.get_request(company, outcome.request_id).await.unwrap();
    assert_eq!(request.current_step, Some(2));
    assert_eq!(request.status, ApprovalStatus::Pending);

    let verdict = svc
        .decide(decide_as(company, outcome.request_id, 2, approver2, true))
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Approved);
}

// ─── 6: delegation pre-resolves at file time ─────────────────────────────────

#[tokio::test]
async fn live_delegation_pre_resolves_at_file_time() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let approver = Uuid::new_v4();
    let delegate = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        Some(approver),
        None,
    )
    .await;

    let today = chrono::Utc::now().date_naive();
    let svc = engine(&pool);

    // A window that ended yesterday does not re-route the chain.
    seed_delegation(
        &pool,
        company,
        approver,
        delegate,
        today - chrono::Duration::days(10),
        today - chrono::Duration::days(1),
    )
    .await;
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    let (status, delegated_from) = step_row(&pool, company, outcome.request_id, approver).await;
    assert_eq!(status, "pending");
    assert!(
        delegated_from.is_none(),
        "an expired window leaves the row unrouted"
    );
    let err = svc
        .decide(decide_as(company, outcome.request_id, 1, delegate, true))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::NotStepApprover));

    // A live window re-routes the chain to the delegate, stamped with the source.
    seed_delegation(
        &pool,
        company,
        approver,
        delegate,
        today,
        today + chrono::Duration::days(5),
    )
    .await;
    let outcome2 = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    let (status, delegated_from) = step_row(&pool, company, outcome2.request_id, delegate).await;
    assert_eq!(status, "pending");
    assert_eq!(delegated_from, Some(approver));

    // The delegate decides; the stamp records whose authority was used (not the decider).
    let verdict = svc
        .decide(decide_as(company, outcome2.request_id, 1, delegate, true))
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Approved);
    let (_, delegated_from) = step_row(&pool, company, outcome2.request_id, delegate).await;
    assert_eq!(delegated_from, Some(approver));
}

// ─── 7: decide-time delegation stamps inherited authority ────────────────────

#[tokio::test]
async fn decide_time_delegation_stamps_the_authority_source() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let approver = Uuid::new_v4();
    let delegate = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        Some(approver),
        None,
    )
    .await;
    let svc = engine(&pool);

    // Chain materialized BEFORE the window existed: the row still belongs to the approver.
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    let (status, _) = step_row(&pool, company, outcome.request_id, approver).await;
    assert_eq!(status, "pending");

    // The window opens after filing — decide-time authorization accepts the delegate.
    let today = chrono::Utc::now().date_naive();
    seed_delegation(
        &pool,
        company,
        approver,
        delegate,
        today,
        today + chrono::Duration::days(5),
    )
    .await;
    let verdict = svc
        .decide(decide_as(company, outcome.request_id, 1, delegate, true))
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Approved);

    // The row records the authority source (the assignee), not the acting delegate.
    let (status, delegated_from) = step_row(&pool, company, outcome.request_id, approver).await;
    assert_eq!(status, "approved");
    assert_eq!(delegated_from, Some(approver));
}

// ─── 8: dynamic kinds resolve through the host resolver ──────────────────────

/// Resolves dynamic kinds from fixed values — the shape a real host resolver
/// derives from org structure. Manager/department-head are single employees;
/// roles/positions resolve to every current holder.
struct MapResolver {
    manager: Uuid,
    department_head: Uuid,
    roles: std::collections::HashMap<Uuid, Vec<Uuid>>,
    positions: std::collections::HashMap<Uuid, Vec<Uuid>>,
}

impl MapResolver {
    fn new(manager: Uuid, department_head: Uuid) -> Self {
        Self {
            manager,
            department_head,
            roles: Default::default(),
            positions: Default::default(),
        }
    }

    fn with_role(mut self, role: Uuid, holders: Vec<Uuid>) -> Self {
        self.roles.insert(role, holders);
        self
    }

    fn with_position(mut self, position: Uuid, holders: Vec<Uuid>) -> Self {
        self.positions.insert(position, holders);
        self
    }
}

#[async_trait::async_trait]
impl ApproverResolver for MapResolver {
    async fn manager_of(&self, _c: Uuid, _r: Uuid) -> Result<Uuid, String> {
        Ok(self.manager)
    }
    async fn department_head_of(&self, _c: Uuid, _r: Uuid) -> Result<Uuid, String> {
        Ok(self.department_head)
    }
    async fn role_holders(&self, _c: Uuid, role: Uuid) -> Result<Vec<Uuid>, String> {
        Ok(self.roles.get(&role).cloned().unwrap_or_default())
    }
    async fn position_holders(&self, _c: Uuid, position: Uuid) -> Result<Vec<Uuid>, String> {
        Ok(self.positions.get(&position).cloned().unwrap_or_default())
    }
}

/// The (kind, ref) a step row was materialized from, by its assignee.
async fn step_kind_ref(
    pool: &PgPool,
    company: Uuid,
    request: Uuid,
    assigned_to: Uuid,
) -> (String, Option<Uuid>) {
    let row: (String, Option<Uuid>) = sqlx::query_as(
        r#"SELECT approver_kind::text, approver_ref FROM approvals.approval_steps
            WHERE company_id = $1 AND request_id = $2 AND assigned_to = $3
              AND (metadata->>'deleted_at') IS NULL"#,
    )
    .bind(company)
    .bind(request)
    .bind(assigned_to)
    .fetch_one(pool)
    .await
    .unwrap();
    row
}

#[tokio::test]
async fn resolver_materializes_the_dynamic_chain_once() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let manager = Uuid::new_v4();
    let head = Uuid::new_v4();
    let role = Uuid::new_v4();
    let role_holder = Uuid::new_v4();
    let position = Uuid::new_v4();
    let position_holder = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "manager_of_requester",
        None,
        None,
    )
    .await;
    seed_template(&pool, company, policy, 2, "department_head", None, None).await;
    seed_template(&pool, company, policy, 3, "role", Some(role), None).await;
    seed_template(&pool, company, policy, 4, "position", Some(position), None).await;

    let svc = ApprovalsWriteService::new(pool.clone()).with_resolver(Arc::new(
        MapResolver::new(manager, head)
            .with_role(role, vec![role_holder])
            .with_position(position, vec![position_holder]),
    ));
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    assert_eq!(outcome.verdict, ApprovalStatus::Pending);
    assert_eq!(count_steps(&pool, company, outcome.request_id).await, 4);

    // Single-approver kinds carry no ref; multi-holder kinds keep the template id
    // (the identity decide-time semantics key on).
    assert_eq!(
        step_kind_ref(&pool, company, outcome.request_id, manager).await,
        ("manager_of_requester".into(), None)
    );
    assert_eq!(
        step_kind_ref(&pool, company, outcome.request_id, head).await,
        ("department_head".into(), None)
    );
    assert_eq!(
        step_kind_ref(&pool, company, outcome.request_id, role_holder).await,
        ("role".into(), Some(role))
    );
    assert_eq!(
        step_kind_ref(&pool, company, outcome.request_id, position_holder).await,
        ("position".into(), Some(position))
    );
}

#[tokio::test]
async fn role_step_materializes_one_row_per_holder() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let role = Uuid::new_v4();
    let h1 = Uuid::new_v4();
    let h2 = Uuid::new_v4();
    let h3 = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(&pool, company, policy, 1, "role", Some(role), None).await;

    let svc = ApprovalsWriteService::new(pool.clone()).with_resolver(Arc::new(
        MapResolver::new(Uuid::new_v4(), Uuid::new_v4()).with_role(role, vec![h1, h2, h3]),
    ));
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    assert_eq!(count_steps(&pool, company, outcome.request_id).await, 3);
    for h in [h1, h2, h3] {
        assert_eq!(
            step_kind_ref(&pool, company, outcome.request_id, h).await,
            ("role".into(), Some(role)),
            "every holder row carries the same template identity"
        );
    }
}

#[tokio::test]
async fn any_holder_first_approve_completes_and_skips_siblings() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let role = Uuid::new_v4();
    let h1 = Uuid::new_v4();
    let h2 = Uuid::new_v4();
    let h3 = Uuid::new_v4();
    let step2 = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(&pool, company, policy, 1, "role", Some(role), None).await;
    seed_template(
        &pool,
        company,
        policy,
        2,
        "specific_employee",
        Some(step2),
        None,
    )
    .await;

    let svc = ApprovalsWriteService::new(pool.clone()).with_resolver(Arc::new(
        MapResolver::new(Uuid::new_v4(), Uuid::new_v4()).with_role(role, vec![h1, h2, h3]),
    ));
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();

    // ONE holder approves: the step completes, the chain advances.
    let verdict = svc
        .decide(decide_as(company, outcome.request_id, 1, h1, true))
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Pending, "chain advances to step 2");
    let request = svc.get_request(company, outcome.request_id).await.unwrap();
    assert_eq!(request.current_step, Some(2));

    // The sibling rows are recorded skipped — the row answers why its holder
    // never decided.
    for h in [h2, h3] {
        assert_eq!(
            step_row(&pool, company, outcome.request_id, h).await.0,
            "skipped"
        );
    }
    assert_eq!(
        step_row(&pool, company, outcome.request_id, h1).await.0,
        "approved"
    );

    // A late sibling decide is the usual out-of-turn 409, not a state change.
    let err = svc
        .decide(decide_as(company, outcome.request_id, 1, h2, true))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::OutOfTurn));

    // The chain still finishes through step 2.
    let verdict = svc
        .decide(decide_as(company, outcome.request_id, 2, step2, true))
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Approved);
}

#[tokio::test]
async fn any_holder_reject_fails_the_request() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let role = Uuid::new_v4();
    let h1 = Uuid::new_v4();
    let h2 = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(&pool, company, policy, 1, "role", Some(role), None).await;

    let svc = ApprovalsWriteService::new(pool.clone()).with_resolver(Arc::new(
        MapResolver::new(Uuid::new_v4(), Uuid::new_v4()).with_role(role, vec![h1, h2]),
    ));
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();

    let verdict = svc
        .decide(decide_as(company, outcome.request_id, 1, h2, false))
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Rejected);
    assert_eq!(
        step_row(&pool, company, outcome.request_id, h1).await.0,
        "skipped"
    );
    assert_eq!(
        step_row(&pool, company, outcome.request_id, h2).await.0,
        "rejected"
    );
}

#[tokio::test]
async fn position_steps_are_any_holder_too() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let position = Uuid::new_v4();
    let p1 = Uuid::new_v4();
    let p2 = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(&pool, company, policy, 1, "position", Some(position), None).await;

    let svc = ApprovalsWriteService::new(pool.clone()).with_resolver(Arc::new(
        MapResolver::new(Uuid::new_v4(), Uuid::new_v4()).with_position(position, vec![p1, p2]),
    ));
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    assert_eq!(count_steps(&pool, company, outcome.request_id).await, 2);

    let verdict = svc
        .decide(decide_as(company, outcome.request_id, 1, p2, true))
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Approved);
    assert_eq!(
        step_row(&pool, company, outcome.request_id, p1).await.0,
        "skipped"
    );
    assert_eq!(
        step_row(&pool, company, outcome.request_id, p2).await.0,
        "approved"
    );
}

#[tokio::test]
async fn role_with_no_holders_fails_closed() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let role = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(&pool, company, policy, 1, "role", Some(role), None).await;

    // The resolver knows the role — it just has no holders right now.
    let svc = ApprovalsWriteService::new(pool.clone()).with_resolver(Arc::new(
        MapResolver::new(Uuid::new_v4(), Uuid::new_v4()).with_role(role, vec![]),
    ));
    let err = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::InvalidChain(_)), "{err}");
    assert_eq!(err.code(), "invalid_approval_chain");
    assert_eq!(err.http_status(), 422);

    let n: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM approvals.approval_requests WHERE company_id = $1"#,
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0, "a holder-less role leaves no request behind");
}

#[tokio::test]
async fn second_live_template_at_one_step_no_is_refused() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "role",
        Some(Uuid::new_v4()),
        None,
    )
    .await;

    // The partial unique idx_approval_step_templates_policy_id_step_no keeps ONE
    // live template per (policy, step_no): a step number is always a single
    // template's regime — one named approver, one all_of quorum, or one
    // any-holder group. Composition across regimes at one number is an
    // authoring-time refusal (compose with more step numbers, not stacked
    // templates), not a state the engine ever has to reconcile.
    let err = pool
        .execute(
            sqlx::query(
                r#"INSERT INTO approvals.approval_step_templates
                       (id, company_id, policy_id, step_no, approver_kind, approver_ref,
                        all_of, sla_hours, metadata)
                   VALUES ($1, $2, $3, 1, 'role', $4, NULL, NULL, '{}'::jsonb)"#,
            )
            .bind(Uuid::new_v4())
            .bind(company)
            .bind(policy)
            .bind(Uuid::new_v4()),
        )
        .await
        .unwrap_err();
    let code = err
        .as_database_error()
        .and_then(|d| d.code())
        .map(|c| c.to_string())
        .unwrap_or_default();
    assert_eq!(
        code, "23505",
        "a second live template at one step is refused"
    );

    // Soft-deleting the first frees the slot — replacing a step is deactivate-then-add,
    // the same discipline the single-active-policy index imposes one level up.
    sqlx::query(
        r#"UPDATE approvals.approval_step_templates
              SET metadata = metadata || '{"deleted_at": "2026-01-01T00:00:00Z"}'::jsonb
            WHERE company_id = $1 AND policy_id = $2"#,
    )
    .bind(company)
    .bind(policy)
    .execute(&pool)
    .await
    .unwrap();
    pool.execute(
        sqlx::query(
            r#"INSERT INTO approvals.approval_step_templates
                   (id, company_id, policy_id, step_no, approver_kind, approver_ref,
                    all_of, sla_hours, metadata)
               VALUES ($1, $2, $3, 1, 'role', $4, NULL, NULL, '{}'::jsonb)"#,
        )
        .bind(Uuid::new_v4())
        .bind(company)
        .bind(policy)
        .bind(Uuid::new_v4()),
    )
    .await
    .unwrap();
}

/// Multi-holder surface keeps the one-decider-per-row invariant: when every holder
/// of a role delegates to the SAME person, the step materializes two rows for one
/// decider — a chain nobody else could ever contest. Filing refuses it as the same
/// typed configuration fault a duplicated quorum member produces.
#[tokio::test]
async fn holders_sharing_one_delegate_fail_at_file() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let role = Uuid::new_v4();
    let h1 = Uuid::new_v4();
    let h2 = Uuid::new_v4();
    let shared_delegate = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(&pool, company, policy, 1, "role", Some(role), None).await;
    seed_delegation(
        &pool,
        company,
        h1,
        shared_delegate,
        today() - chrono::Duration::days(1),
        today() + chrono::Duration::days(7),
    )
    .await;
    seed_delegation(
        &pool,
        company,
        h2,
        shared_delegate,
        today() - chrono::Duration::days(1),
        today() + chrono::Duration::days(7),
    )
    .await;

    let svc = ApprovalsWriteService::new(pool.clone()).with_resolver(Arc::new(
        MapResolver::new(Uuid::new_v4(), Uuid::new_v4()).with_role(role, vec![h1, h2]),
    ));
    let err = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::InvalidChain(_)), "{err}");
    assert_eq!(err.http_status(), 422);

    let n: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM approvals.approval_requests WHERE company_id = $1"#,
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0, "a colliding delegation set leaves no request behind");
}

// ─── 9: cross-tenant is 404, both directions ─────────────────────────────────

#[tokio::test]
async fn cross_tenant_ids_are_404s_not_leakage() {
    let pool = pool().await;
    let company_a = Uuid::new_v4();
    let company_b = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let policy = seed_policy(&pool, company_a, "leave").await;
    seed_template(
        &pool,
        company_a,
        policy,
        1,
        "specific_employee",
        Some(Uuid::new_v4()),
        None,
    )
    .await;
    let svc = engine(&pool);

    let outcome = svc
        .file(filing(company_a, Uuid::new_v4(), employee))
        .await
        .unwrap();

    for verb in ["get", "decide", "withdraw"] {
        let result = match verb {
            "get" => svc
                .get_request(company_b, outcome.request_id)
                .await
                .map(|_| ())
                .map_err(|e| e.code().to_string()),
            "decide" => svc
                .decide(decide_as(company_b, outcome.request_id, 1, employee, true))
                .await
                .map(|_| ())
                .map_err(|e| e.code().to_string()),
            _ => svc
                .withdraw(company_b, outcome.request_id, employee)
                .await
                .map(|_| ())
                .map_err(|e| e.code().to_string()),
        };
        assert_eq!(
            result.unwrap_err(),
            "approval_request_not_found",
            "{verb} across the tenant fence must 404"
        );
    }

    // And the owner tenant still sees its row — the fence is a 404, not corruption.
    assert!(svc.get_request(company_a, outcome.request_id).await.is_ok());
}

// ─── 10: filing idempotency, including the concurrent loser ──────────────────

#[tokio::test]
async fn concurrent_filings_converge_on_one_request() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let resource = Uuid::new_v4();
    let svc = Arc::new(engine(&pool));

    let (o1, o2, o3, o4) = tokio::join!(
        svc.file(filing(company, resource, employee)),
        svc.file(filing(company, resource, employee)),
        svc.file(filing(company, resource, employee)),
        svc.file(filing(company, resource, employee)),
    );
    let outcomes = [o1.unwrap(), o2.unwrap(), o3.unwrap(), o4.unwrap()];

    let ids: std::collections::HashSet<Uuid> = outcomes.iter().map(|o| o.request_id).collect();
    assert_eq!(ids.len(), 1, "every concurrent filing lands on one request");
    assert!(
        outcomes.iter().any(|o| !o.already_filed),
        "at least one filing created the row"
    );

    // The winner's steps materialized exactly once (no duplicated quorum rows).
    let id = outcomes[0].request_id;
    assert_eq!(
        count_steps(&pool, company, id).await,
        0,
        "no policy: zero steps"
    );
}

// ─── 11: withdraw frees the resource for a fresh chain ────────────────────────

#[tokio::test]
async fn withdraw_then_refile_files_a_fresh_chain() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let approver = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        Some(approver),
        None,
    )
    .await;
    let svc = engine(&pool);

    let resource = Uuid::new_v4();
    let outcome = svc.file(filing(company, resource, employee)).await.unwrap();

    // Not the requester: refused.
    let err = svc
        .withdraw(company, outcome.request_id, Uuid::new_v4())
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::NotRequester));

    let verdict = svc
        .withdraw(company, outcome.request_id, employee)
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Withdrawn);

    // The withdrawn chain is soft-deleted — the same resource files a NEW chain.
    let refiled = svc.file(filing(company, resource, employee)).await.unwrap();
    assert_ne!(refiled.request_id, outcome.request_id);
    assert_eq!(refiled.verdict, ApprovalStatus::Pending);
    assert_eq!(count_steps(&pool, company, refiled.request_id).await, 1);
}

// ─── 12: dynamic kinds fail closed without a resolver ────────────────────────

#[tokio::test]
async fn dynamic_approver_kinds_fail_closed() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "manager_of_requester",
        None,
        None,
    )
    .await;

    // The shipped default resolves nothing — prove it by behavior, not type.
    let svc = ApprovalsWriteService::new(pool.clone());
    let err = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::StepResolutionFailed(_)));
    assert_eq!(err.http_status(), 422);

    // No request row was created — the failure left nothing behind.
    let n: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM approvals.approval_requests WHERE company_id = $1"#,
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}

// ─── 13: the RLS fence under SET ROLE ────────────────────────────────────────

#[tokio::test]
async fn set_role_fence_binds_to_the_company() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let svc = engine(&pool);

    // One live request row for this company.
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    let _ = outcome;

    // The suite connects as the DB owner (superuser — RLS never binds). Production runs
    // as a plain non-superuser; probe that posture with SET ROLE on one connection.
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query(
        r#"DO $$ BEGIN
               IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'approvals_probe_rls') THEN
                   CREATE ROLE approvals_probe_rls NOLOGIN;
               END IF;
           END $$"#,
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("GRANT USAGE ON SCHEMA approvals TO approvals_probe_rls")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("GRANT SELECT ON ALL TABLES IN SCHEMA approvals TO approvals_probe_rls")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("SET ROLE approvals_probe_rls")
        .execute(&mut *conn)
        .await
        .unwrap();

    // Unbound (no tenant): zero rows by design — the fence default.
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM approvals.approval_requests")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(n, 0, "unbound non-superuser sees zero rows");

    // Bound to the company (request-scoped set_config, transaction-local like the app):
    // exactly its company's rows.
    let mut tx = conn.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.company_id', $1, true)")
        .bind(company.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM approvals.approval_requests")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(n, 1, "bound connection sees exactly its company's rows");
    tx.rollback().await.unwrap();

    sqlx::query("RESET ROLE").execute(&mut *conn).await.unwrap();
}

// ─── 14: the guarded routes behind real company_auth ─────────────────────────

#[tokio::test]
async fn guarded_routes_decide_with_engine_side_authorization() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let approver = Uuid::new_v4();
    let outsider = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        Some(approver),
        None,
    )
    .await;

    let svc = engine(&pool);
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    let m = module(&pool).await;
    let app = create_guarded_approvals_routes(&m);

    // GET the request as the requester.
    let token = token_for(company, employee);
    let (status, body) = req(
        app.clone(),
        "GET",
        &format!("/approvals/requests/{}", outcome.request_id),
        &token,
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"status\":\"pending\""), "body: {body}");

    // A non-approver decide: engine-side 403 with the stable code.
    let token = token_for(company, outsider);
    let (status, body) = req(
        app.clone(),
        "POST",
        &format!("/approvals/requests/{}/decide", outcome.request_id),
        &token,
        r#"{"stepNo":1,"decision":"approve"}"#.into(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("not_step_approver"), "body: {body}");

    // Garbage decision verb: 422, not 500.
    let token = token_for(company, approver);
    let (status, _) = req(
        app.clone(),
        "POST",
        &format!("/approvals/requests/{}/decide", outcome.request_id),
        &token,
        r#"{"stepNo":1,"decision":"maybe"}"#.into(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // The assigned approver decides: 200 with the resulting status.
    let (status, body) = req(
        app.clone(),
        "POST",
        &format!("/approvals/requests/{}/decide", outcome.request_id),
        &token,
        r#"{"stepNo":1,"decision":"approve","comment":"probe"}"#.into(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"status\":\"approved\""), "body: {body}");

    // Withdraw by someone who is not the requester: 403.
    let token = token_for(company, outsider);
    let (status, _) = req(
        app.clone(),
        "POST",
        &format!("/approvals/requests/{}/withdraw", outcome.request_id),
        &token,
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Cross-tenant: a valid token for another company sees 404.
    let other_company = Uuid::new_v4();
    let token = token_for(other_company, employee);
    let (status, _) = req(
        app,
        "GET",
        &format!("/approvals/requests/{}", outcome.request_id),
        &token,
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A decide body carrying `roleRefs` (the pre-consent authorization channel this
/// engine removed) is inert: the field no longer exists on the wire shape, and a
/// non-assignee presenting it authorizes nothing — engine-side 403, stable code.
#[tokio::test]
async fn decide_ignores_presented_role_refs() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let approver = Uuid::new_v4();
    let claimant = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        Some(approver),
        None,
    )
    .await;

    let svc = engine(&pool);
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    let m = module(&pool).await;
    let app = create_guarded_approvals_routes(&m);

    // The claimant "holds" a role id and presents it as their authorization.
    let token = token_for(company, claimant);
    let (status, body) = req(
        app,
        "POST",
        &format!("/approvals/requests/{}/decide", outcome.request_id),
        &token,
        r#"{"stepNo":1,"decision":"approve","roleRefs":["11111111-1111-1111-1111-111111111111"]}"#
            .into(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body.contains("not_step_approver"), "body: {body}");

    // The step is untouched.
    assert_eq!(
        step_row(&pool, company, outcome.request_id, approver)
            .await
            .0,
        "pending"
    );
}

// ─── 15: delegation is a principal-verb, not master data ─────────────────────
//
// A delegation row carries real authority — a live window lets the delegate decide
// the approver's steps. The guarded surface exposes the lifecycle ONLY as
// self-service verbs stamping the principal from the token: a body approverId is
// inert, revoke is approver-only and row-truth, and revocation stops decide-time
// authorization without re-routing rows a filing already resolved.

fn today() -> chrono::NaiveDate {
    chrono::Utc::now().date_naive()
}

#[tokio::test]
async fn delegation_verb_stamps_the_principal_from_the_token() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let approver = Uuid::new_v4();
    let delegate = Uuid::new_v4();
    let impostor = Uuid::new_v4();
    let app = create_guarded_approvals_routes_with(std::sync::Arc::new(engine(&pool)));

    // A foreign approverId in the body grants nothing — the row stamps the token's sub.
    let (status, body) = req(
        app.clone(),
        "POST",
        "/approvals/delegations",
        &token_for(company, approver),
        serde_json::json!({
            "approverId": impostor,
            "delegateTo": delegate,
            "validFrom": today(),
            "validTo": today() + chrono::Duration::days(7),
            "reason": "probe"
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    let id: Uuid = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    let (row_approver, source): (Uuid, String) = sqlx::query_as(
        r#"SELECT approver_id, metadata->>'source' FROM approvals.delegations WHERE id = $1"#,
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row_approver, approver, "the principal is the token's sub");
    assert_eq!(source, "self_service");

    // Self-delegation: 422, stable code.
    let (status, body) = req(
        app.clone(),
        "POST",
        "/approvals/delegations",
        &token_for(company, approver),
        serde_json::json!({
            "delegateTo": approver,
            "validFrom": today(),
            "validTo": today() + chrono::Duration::days(7)
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");
    assert!(body.contains("self_delegation_refused"), "body: {body}");

    // Inverted window: 422.
    let (status, body) = req(
        app,
        "POST",
        "/approvals/delegations",
        &token_for(company, approver),
        serde_json::json!({
            "delegateTo": delegate,
            "validFrom": today() + chrono::Duration::days(7),
            "validTo": today()
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "body: {body}");
    assert!(body.contains("delegation_window_invalid"), "body: {body}");
}

#[tokio::test]
async fn delegation_revoke_discipline_and_post_revoke_semantics() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let approver = Uuid::new_v4();
    let delegate = Uuid::new_v4();
    let stranger = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        Some(approver),
        None,
    )
    .await;

    let svc = engine(&pool);
    let app = create_guarded_approvals_routes_with(std::sync::Arc::new(svc.clone()));
    let (status, body) = req(
        app.clone(),
        "POST",
        "/approvals/delegations",
        &token_for(company, approver),
        serde_json::json!({
            "delegateTo": delegate,
            "validFrom": today() - chrono::Duration::days(1),
            "validTo": today() + chrono::Duration::days(7)
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "body: {body}");
    let id: Uuid = serde_json::from_str::<serde_json::Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    // While live: a filing pre-resolves the step onto the delegate.
    let live = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    let (assigned, inherited) = step_row(&pool, company, live.request_id, delegate).await;
    assert_eq!(assigned, "pending");
    assert_eq!(inherited, Some(approver));

    // Stranger revoke: 403, stable code.
    let (status, body) = req(
        app.clone(),
        "POST",
        &format!("/approvals/delegations/{id}/revoke"),
        &token_for(company, stranger),
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "body: {body}");
    assert!(body.contains("not_delegation_approver"), "body: {body}");

    // Approver revoke: 200, and the row flips.
    let (status, body) = req(
        app.clone(),
        "POST",
        &format!("/approvals/delegations/{id}/revoke"),
        &token_for(company, approver),
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    assert!(body.contains("\"status\":\"revoked\""), "body: {body}");

    // Double revoke: row-truth 409, not a silent second success.
    let (status, body) = req(
        app.clone(),
        "POST",
        &format!("/approvals/delegations/{id}/revoke"),
        &token_for(company, approver),
        String::new(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "body: {body}");
    assert!(body.contains("delegation_not_active"), "body: {body}");

    // The pre-resolved row from the live window STANDS: delegation resolves once at
    // file, so the delegate still decides it (delegated_from already stamps provenance).
    let verdict = svc
        .decide(decide_as(company, live.request_id, 1, delegate, true))
        .await
        .unwrap();
    assert_eq!(verdict, ApprovalStatus::Approved);

    // A filing AFTER the revoke resolves onto the approver again…
    let fresh = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    let (assigned, inherited) = step_row(&pool, company, fresh.request_id, approver).await;
    assert_eq!(assigned, "pending");
    assert_eq!(inherited, None);

    // …and the delegate no longer holds decide-time authority over it: 403.
    let err = svc
        .decide(decide_as(company, fresh.request_id, 1, delegate, true))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::NotStepApprover), "{err}");
}

// ─── 16: chain validity is enforced at file time ─────────────────────────────
//
// The engine walks step numbers from 1 one at a time. A chain that is empty, starts
// above 1, skips a number, or resolves two members of one step onto the same approver
// would park the request on a step nobody can decide — every decision a 404, withdraw
// the only exit. All of those are configuration faults; filing must reject them typed
// (422) and leave no request row behind.

#[tokio::test]
async fn policy_without_templates_fails_at_file() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    let _ = policy;

    let svc = ApprovalsWriteService::new(pool.clone());
    let err = svc
        .file(filing(company, Uuid::new_v4(), Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::InvalidChain(_)), "{err}");
    assert_eq!(err.http_status(), 422);
    assert_eq!(err.code(), "invalid_approval_chain");

    let n: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM approvals.approval_requests WHERE company_id = $1"#,
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn chain_with_a_step_gap_fails_at_file() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        Some(Uuid::new_v4()),
        None,
    )
    .await;
    seed_template(
        &pool,
        company,
        policy,
        3,
        "specific_employee",
        Some(Uuid::new_v4()),
        None,
    )
    .await;

    let svc = ApprovalsWriteService::new(pool.clone());
    let err = svc
        .file(filing(company, Uuid::new_v4(), Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::InvalidChain(_)), "{err}");
    assert_eq!(err.http_status(), 422);
}

#[tokio::test]
async fn chain_not_starting_at_one_fails_at_file() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        2,
        "specific_employee",
        Some(Uuid::new_v4()),
        None,
    )
    .await;

    let svc = ApprovalsWriteService::new(pool.clone());
    let err = svc
        .file(filing(company, Uuid::new_v4(), Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::InvalidChain(_)), "{err}");
    assert_eq!(err.http_status(), 422);
}

#[tokio::test]
async fn quorum_naming_a_member_twice_fails_at_file() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let duplicated = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        None,
        Some(serde_json::json!([duplicated, duplicated])),
    )
    .await;

    let svc = ApprovalsWriteService::new(pool.clone());
    let err = svc
        .file(filing(company, Uuid::new_v4(), Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::InvalidChain(_)), "{err}");
    assert_eq!(err.http_status(), 422);
}

#[tokio::test]
async fn quorum_sharing_one_delegate_fails_at_file() {
    // Two distinct members whose live delegation windows both point at the same person:
    // the step would carry two rows with one decider — reject it as a chain fault.
    let pool = pool().await;
    let company = Uuid::new_v4();
    let member_a = Uuid::new_v4();
    let member_b = Uuid::new_v4();
    let shared_delegate = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(
        &pool,
        company,
        policy,
        1,
        "specific_employee",
        None,
        Some(serde_json::json!([member_a, member_b])),
    )
    .await;
    let window = (
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    );
    seed_delegation(
        &pool,
        company,
        member_a,
        shared_delegate,
        window.0,
        window.1,
    )
    .await;
    seed_delegation(
        &pool,
        company,
        member_b,
        shared_delegate,
        window.0,
        window.1,
    )
    .await;

    let svc = ApprovalsWriteService::new(pool.clone());
    let err = svc
        .file(filing(company, Uuid::new_v4(), Uuid::new_v4()))
        .await
        .unwrap_err();
    assert!(matches!(err, ApprovalsError::InvalidChain(_)), "{err}");
    assert_eq!(err.http_status(), 422);

    let n: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM approvals.approval_requests WHERE company_id = $1"#,
    )
    .bind(company)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}

// ─── 17: one active policy per resource ──────────────────────────────────────
//
// The partial unique index approval_policies_single_active refuses a second active
// policy for the same (company, resource_type) — a replacement policy must deactivate
// the old one first, or it silently never routes anything.

#[tokio::test]
async fn second_active_policy_for_a_resource_is_refused() {
    let pool = pool().await;
    let company = Uuid::new_v4();

    let insert = |name: &str, active: bool| {
        sqlx::query(
            r#"INSERT INTO approvals.approval_policies
                   (id, company_id, resource_type, name, is_active, description, metadata)
               VALUES ($1, $2, 'leave', $3, $4, NULL, '{}'::jsonb)"#,
        )
        .bind(Uuid::new_v4())
        .bind(company)
        .bind(name.to_string())
        .bind(active)
    };
    pool.execute(insert("first policy", true)).await.unwrap();
    // Same name family, distinct names — the (company, resource_type, name) unique is not
    // the constraint under test; the partial unique on ACTIVE rows is.
    let err = pool.execute(insert("replacement", true)).await.unwrap_err();
    let code = err
        .as_database_error()
        .and_then(|d| d.code())
        .map(|c| c.to_string())
        .unwrap_or_default();
    assert_eq!(
        code, "23505",
        "second active policy must trip the partial unique"
    );

    // Deactivating the first frees the slot — the replacement activates cleanly.
    sqlx::query("UPDATE approvals.approval_policies SET is_active = false WHERE company_id = $1")
        .bind(company)
        .execute(&pool)
        .await
        .unwrap();
    pool.execute(insert("replacement", true)).await.unwrap();
}
