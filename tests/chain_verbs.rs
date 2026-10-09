//! A policy's chain written and previewed whole.
//!
//! What the suite pins:
//! - the shape a chain must have is checked before anything is written;
//! - `replace_chain` numbers the steps 1..n, retires the old ones, and either writes the
//!   whole chain or leaves the old one as it was;
//! - a filing after a replace walks the new chain;
//! - `preview_chain` says per step who decides, with delegation applied, and why a step
//!   would reach nobody, without writing anything.
//!
//! DB: DATABASE_URL wins, else the module's local test DB.

use sqlx::PgPool;
use uuid::Uuid;

use backbone_approvals::application::service::{
    check_chain, ApprovalsError, ApprovalsWriteService, ChainStep, FileFiling,
};
use backbone_approvals::{ApprovalPriority, ApprovalResourceType, ApprovalStatus, ApproverKind};

/// Filing reads THE active policy of a resource type; tests that file run one at a time.
/// An async mutex, since the guard is held across the test's awaits.
static SEQUENTIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://serpa:serpa_dev_password@127.0.0.1:5432/backbone_approvals_test".into()
    });
    PgPool::connect(&url).await.unwrap()
}

async fn sequential() -> tokio::sync::MutexGuard<'static, ()> {
    SEQUENTIAL.lock().await
}

async fn policy(pool: &PgPool, resource_type: &str, status: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO approvals.approval_policies (id, resource_type, name, status, metadata)
           VALUES ($1, $2::approval_resource_type, $3, $4::approval_policy_status, '{}'::jsonb)"#,
    )
    .bind(id)
    .bind(resource_type)
    .bind(format!("chain probe {id}"))
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    id
}

fn person(id: Uuid) -> ChainStep {
    ChainStep {
        approver_kind: ApproverKind::SpecificEmployee,
        approver_ref: Some(id),
        sla_hours: Some(24),
        all_of: None,
    }
}

async fn live_chain(pool: &PgPool, policy: Uuid) -> Vec<(i32, Option<Uuid>)> {
    sqlx::query_as(
        r#"SELECT step_no, approver_ref FROM approvals.approval_step_templates
            WHERE policy_id = $1 AND (metadata->>'deleted_at') IS NULL ORDER BY step_no"#,
    )
    .bind(policy)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[test]
fn a_chain_is_checked_before_anything_is_written() {
    assert!(matches!(
        check_chain(&[]),
        Err(ApprovalsError::InvalidChain(_))
    ));
    let nobody = ChainStep {
        approver_ref: None,
        ..person(Uuid::new_v4())
    };
    let err = check_chain(&[nobody]).unwrap_err();
    assert!(err.to_string().contains("step 1"), "{err}");
    let late = ChainStep {
        sla_hours: Some(0),
        ..person(Uuid::new_v4())
    };
    assert!(check_chain(&[late]).is_err());
    let dup = Uuid::new_v4();
    let quorum = ChainStep {
        approver_kind: ApproverKind::SpecificEmployee,
        approver_ref: None,
        sla_hours: None,
        all_of: Some(vec![dup, dup]),
    };
    assert!(check_chain(&[quorum]).is_err());
    let manager = ChainStep {
        approver_kind: ApproverKind::ManagerOfRequester,
        approver_ref: None,
        sla_hours: None,
        all_of: None,
    };
    assert!(check_chain(&[manager, person(Uuid::new_v4())]).is_ok());
}

#[tokio::test]
async fn replace_numbers_the_chain_and_retires_the_old_one() {
    let pool = pool().await;
    let svc = ApprovalsWriteService::new(pool.clone());
    let p = policy(&pool, "leave", "inactive").await;
    let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());

    svc.replace_chain(p, vec![person(a), person(b)])
        .await
        .unwrap();
    assert_eq!(live_chain(&pool, p).await, vec![(1, Some(a)), (2, Some(b))]);

    // Reordered and grown: step numbers are reused, which one live template per
    // (policy, step_no) would refuse row by row.
    let written = svc
        .replace_chain(p, vec![person(c), person(b), person(a)])
        .await
        .unwrap();
    assert_eq!(written.len(), 3);
    assert_eq!(
        live_chain(&pool, p).await,
        vec![(1, Some(c)), (2, Some(b)), (3, Some(a))]
    );
    let retired: i64 = sqlx::query_scalar(
        r#"SELECT count(*) FROM approvals.approval_step_templates
            WHERE policy_id = $1 AND (metadata->>'deleted_at') IS NOT NULL"#,
    )
    .bind(p)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retired, 2, "the old chain is retired, not erased");
}

#[tokio::test]
async fn a_refused_replace_leaves_the_old_chain_whole() {
    let pool = pool().await;
    let svc = ApprovalsWriteService::new(pool.clone());
    let p = policy(&pool, "leave", "inactive").await;
    let a = Uuid::new_v4();
    svc.replace_chain(p, vec![person(a)]).await.unwrap();

    let broken = ChainStep {
        approver_ref: None,
        ..person(Uuid::new_v4())
    };
    let err = svc
        .replace_chain(p, vec![person(Uuid::new_v4()), broken])
        .await
        .unwrap_err();
    assert_eq!(err.http_status(), 422);
    assert_eq!(live_chain(&pool, p).await, vec![(1, Some(a))]);

    let unknown = svc
        .replace_chain(Uuid::new_v4(), vec![person(a)])
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), "invalid_approval_chain");
}

#[tokio::test]
async fn a_filing_after_a_replace_walks_the_new_chain() {
    let _s = sequential().await;
    let pool = pool().await;
    sqlx::query(
        r#"UPDATE approvals.approval_policies SET status = 'inactive'
            WHERE resource_type = 'expense'::approval_resource_type AND status = 'active'"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    let svc = ApprovalsWriteService::new(pool.clone());
    let p = policy(&pool, "expense", "active").await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    svc.replace_chain(p, vec![person(a)]).await.unwrap();
    svc.replace_chain(p, vec![person(b), person(a)])
        .await
        .unwrap();

    let out = svc
        .file(FileFiling {
            resource_type: ApprovalResourceType::Expense,
            resource_id: Uuid::new_v4(),
            requested_by: Uuid::new_v4(),
            priority: ApprovalPriority::Normal,
            summary: serde_json::json!({}),
        })
        .await
        .unwrap();
    assert_eq!(out.verdict, ApprovalStatus::Pending);
    let steps: Vec<(i32, Uuid)> = sqlx::query_as(
        "SELECT step_no, assigned_to FROM approvals.approval_steps WHERE request_id = $1 ORDER BY step_no",
    )
    .bind(out.request_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(steps, vec![(1, b), (2, a)]);
}

#[tokio::test]
async fn preview_says_who_decides_and_why_a_step_reaches_nobody() {
    let pool = pool().await;
    let svc = ApprovalsWriteService::new(pool.clone());
    let p = policy(&pool, "leave", "inactive").await;
    let (requester, approver, delegate) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    // A live delegation window hands the approver's steps to the delegate.
    sqlx::query(
        r#"INSERT INTO approvals.delegations
             (id, approver_id, delegate_to_id, valid_from, valid_to, status, metadata)
           VALUES ($1, $2, $3, current_date - 1, current_date + 1,
                   'active'::delegation_status, '{}'::jsonb)"#,
    )
    .bind(Uuid::new_v4())
    .bind(approver)
    .bind(delegate)
    .execute(&pool)
    .await
    .unwrap();

    let manager = ChainStep {
        approver_kind: ApproverKind::ManagerOfRequester,
        approver_ref: None,
        sla_hours: None,
        all_of: None,
    };
    let preview = svc
        .preview_chain(
            p,
            Some(vec![person(approver), manager, person(requester)]),
            requester,
        )
        .await
        .unwrap();
    assert_eq!(preview.len(), 3);
    assert_eq!(preview[0].approvers.len(), 1);
    assert_eq!(preview[0].approvers[0].employee_id, delegate);
    assert_eq!(preview[0].approvers[0].delegated_from, Some(approver));
    assert!(preview[0].problem.is_none());
    // No resolver for a manager in this bench, and no org scope: the step reaches nobody.
    assert!(preview[1].approvers.is_empty());
    assert!(preview[1].problem.is_some());
    assert_eq!(
        preview[2].problem.as_deref(),
        Some("the requester would decide their own request")
    );
    // Nothing was written by a preview.
    assert!(live_chain(&pool, p).await.is_empty());
}
