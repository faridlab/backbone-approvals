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
//! - decide-time authorization is engine-side (assigned / live delegation window /
//!   presented role refs), with delegation stamping `delegated_from` (the authority
//!   source, never the decider);
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
use sqlx::{Acquire, PgPool};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

use backbone_approvals::application::service::{
    ApprovalsError, ApprovalsWriteService, ApproverActor, ApproverResolver, Decision, FileFiling,
};
use backbone_approvals::{
    create_guarded_approvals_routes, ApprovalPriority, ApprovalResourceType, ApprovalStatus,
    ApprovalsModule,
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
        actor: ApproverActor {
            employee_id: actor,
            role_refs: vec![],
        },
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

// ─── 8: role steps authorize on presented role refs ──────────────────────────

/// Resolves every dynamic kind to one fixed employee — the semantics a real host
/// resolver would derive from org structure.
struct FixedResolver(Uuid);
#[async_trait::async_trait]
impl ApproverResolver for FixedResolver {
    async fn manager_of(&self, _c: Uuid, _r: Uuid) -> Result<Uuid, String> {
        Ok(self.0)
    }
    async fn department_head_of(&self, _c: Uuid, _r: Uuid) -> Result<Uuid, String> {
        Ok(self.0)
    }
    async fn role_holder(&self, _c: Uuid, _role: Uuid) -> Result<Uuid, String> {
        Ok(self.0)
    }
    async fn position_holder(&self, _c: Uuid, _p: Uuid) -> Result<Uuid, String> {
        Ok(self.0)
    }
}

#[tokio::test]
async fn role_steps_authorize_on_presented_role_refs() {
    let pool = pool().await;
    let company = Uuid::new_v4();
    let employee = Uuid::new_v4();
    let holder = Uuid::new_v4();
    let role = Uuid::new_v4();
    let policy = seed_policy(&pool, company, "leave").await;
    seed_template(&pool, company, policy, 1, "role", Some(role), None).await;

    let svc =
        ApprovalsWriteService::new(pool.clone()).with_resolver(Arc::new(FixedResolver(holder)));
    let outcome = svc
        .file(filing(company, Uuid::new_v4(), employee))
        .await
        .unwrap();
    // Resolved to the holder, but the role ref is kept for decide-time re-check.
    let (status, _) = step_row(&pool, company, outcome.request_id, holder).await;
    assert_eq!(status, "pending");

    // A random actor without the role: refused.
    let mut d = decide_as(company, outcome.request_id, 1, Uuid::new_v4(), true);
    d.actor.role_refs = vec![Uuid::new_v4()];
    assert!(matches!(
        svc.decide(d).await.unwrap_err(),
        ApprovalsError::NotStepApprover
    ));

    // An actor presenting the very role id the row was resolved from: authorized.
    let mut d = decide_as(company, outcome.request_id, 1, Uuid::new_v4(), true);
    d.actor.role_refs = vec![role];
    let verdict = svc.decide(d).await.unwrap();
    assert_eq!(verdict, ApprovalStatus::Approved);
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
