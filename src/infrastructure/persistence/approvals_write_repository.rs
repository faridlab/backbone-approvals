//! `ApprovalsWriteRepository` — every SQL statement the decision engine issues
//! (hand-authored, user-owned; see `metaphor.codegen.yaml`).
//!
//! Family convention (expenses/timeoff): the service owns the verbs and the transactions;
//! this repo owns the statements. Every call rides the caller's bound connection —
//! `company_scope::bind_company_on` was applied by the service — so the RLS fence scopes
//! each statement, and the explicit `company_id = $n` predicates are belt-and-braces (a
//! cross-tenant id matches zero rows, surfacing as 404, never as leakage).
//!
//! Soft-delete lives in `metadata` JSONB (`deleted_at` key), matching the module's partial
//! indexes: "live row" predicates are `(metadata->>'deleted_at') IS NULL`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::entity::{
    ApprovalPolicy, ApprovalRequest, ApprovalResourceType, ApprovalStatus, ApprovalStep,
    ApprovalStepStatus, ApprovalStepTemplate,
};

pub struct ApprovalsWriteRepository;

impl ApprovalsWriteRepository {
    // ── requests: reads ─────────────────────────────────────────────────────

    /// The LIVE request for one resource (the idempotency probe of `file`).
    pub async fn find_live_request(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        resource_type: &ApprovalResourceType,
        resource_id: Uuid,
    ) -> Result<Option<ApprovalRequest>, sqlx::Error> {
        sqlx::query_as::<_, ApprovalRequest>(
            r#"SELECT * FROM approvals.approval_requests
                WHERE company_id = $1 AND resource_type = $2 AND resource_id = $3
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(company)
        .bind(resource_type)
        .bind(resource_id)
        .fetch_optional(&mut *conn)
        .await
    }

    /// One request row regardless of status (decide/status/withdraw entrypoint).
    pub async fn get_request(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        request_id: Uuid,
    ) -> Result<Option<ApprovalRequest>, sqlx::Error> {
        sqlx::query_as::<_, ApprovalRequest>(
            r#"SELECT * FROM approvals.approval_requests
                WHERE company_id = $1 AND id = $2
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(company)
        .bind(request_id)
        .fetch_optional(&mut *conn)
        .await
    }

    // ── policy / templates ──────────────────────────────────────────────────

    /// The active policy for a resource, deterministically picked when a tenant (against
    /// guidance) keeps several active: earliest created, then lowest id.
    pub async fn find_active_policy(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        resource_type: &ApprovalResourceType,
    ) -> Result<Option<ApprovalPolicy>, sqlx::Error> {
        sqlx::query_as::<_, ApprovalPolicy>(
            r#"SELECT * FROM approvals.approval_policies
                WHERE company_id = $1 AND resource_type = $2 AND is_active
                  AND (metadata->>'deleted_at') IS NULL
                ORDER BY (metadata->>'created_at') NULLS LAST, id
                LIMIT 1"#,
        )
        .bind(company)
        .bind(resource_type)
        .fetch_optional(&mut *conn)
        .await
    }

    /// The chain templates of a policy, in step order.
    pub async fn templates_for_policy(
        &self,
        conn: &mut sqlx::PgConnection,
        policy_id: Uuid,
    ) -> Result<Vec<ApprovalStepTemplate>, sqlx::Error> {
        sqlx::query_as::<_, ApprovalStepTemplate>(
            r#"SELECT * FROM approvals.approval_step_templates
                WHERE policy_id = $1
                  AND (metadata->>'deleted_at') IS NULL
                ORDER BY step_no"#,
        )
        .bind(policy_id)
        .fetch_all(&mut *conn)
        .await
    }

    // ── steps: reads ────────────────────────────────────────────────────────

    /// The LIVE member rows of one step (a quorum step holds several).
    pub async fn live_steps_at(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        request_id: Uuid,
        step_no: i32,
    ) -> Result<Vec<ApprovalStep>, sqlx::Error> {
        sqlx::query_as::<_, ApprovalStep>(
            r#"SELECT * FROM approvals.approval_steps
                WHERE company_id = $1 AND request_id = $2 AND step_no = $3
                  AND (metadata->>'deleted_at') IS NULL
                ORDER BY assigned_to"#,
        )
        .bind(company)
        .bind(request_id)
        .bind(step_no)
        .fetch_all(&mut *conn)
        .await
    }

    /// Pending member rows remaining at a step (the quorum countdown).
    pub async fn count_pending_steps_at(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        request_id: Uuid,
        step_no: i32,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM approvals.approval_steps
                WHERE company_id = $1 AND request_id = $2 AND step_no = $3
                  AND status = 'pending'
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(company)
        .bind(request_id)
        .bind(step_no)
        .fetch_one(&mut *conn)
        .await
    }

    /// The deepest materialized step number (finish-vs-advance check).
    pub async fn max_step_no(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        request_id: Uuid,
    ) -> Result<i32, sqlx::Error> {
        sqlx::query_scalar(
            r#"SELECT COALESCE(MAX(step_no), 0) FROM approvals.approval_steps
                WHERE company_id = $1 AND request_id = $2
                  AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(company)
        .bind(request_id)
        .fetch_one(&mut *conn)
        .await
    }

    // ── delegation ──────────────────────────────────────────────────────────

    /// The delegate currently holding an approver's authority, if a live window covers
    /// today. Window and status checks live in the SQL, not the caller.
    pub async fn find_active_delegation_for(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        approver: Uuid,
    ) -> Result<Option<Uuid>, sqlx::Error> {
        sqlx::query_scalar(
            r#"SELECT delegate_to_id FROM approvals.delegations
                WHERE company_id = $1 AND approver_id = $2 AND status = 'active'
                  AND valid_from <= CURRENT_DATE AND valid_to >= CURRENT_DATE
                  AND (metadata->>'deleted_at') IS NULL
                ORDER BY valid_from DESC
                LIMIT 1"#,
        )
        .bind(company)
        .bind(approver)
        .fetch_optional(&mut *conn)
        .await
    }

    /// Does `delegate` hold a live window from `approver`? (Decide-time authorization.)
    pub async fn delegation_exists_for(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        approver: Uuid,
        delegate: Uuid,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            r#"SELECT EXISTS(
                   SELECT 1 FROM approvals.delegations
                    WHERE company_id = $1 AND approver_id = $2 AND delegate_to_id = $3
                      AND status = 'active'
                      AND valid_from <= CURRENT_DATE AND valid_to >= CURRENT_DATE
                      AND (metadata->>'deleted_at') IS NULL)"#,
        )
        .bind(company)
        .bind(approver)
        .bind(delegate)
        .fetch_one(&mut *conn)
        .await
    }

    // ── writes ──────────────────────────────────────────────────────────────

    /// Insert a request row (the `file` unique-race loser catches 23505 and re-selects).
    pub async fn insert_request(
        &self,
        conn: &mut sqlx::PgConnection,
        request: &ApprovalRequest,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO approvals.approval_requests
                   (id, company_id, resource_type, resource_id, policy_id, requested_by,
                    status, current_step, priority, submitted_at, decided_at, decided_by,
                    summary, metadata)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                       jsonb_build_object('created_by', to_jsonb($6::uuid), 'created_at', to_jsonb($10::timestamptz)))"#,
        )
        .bind(request.id)
        .bind(request.company_id)
        .bind(request.resource_type)
        .bind(request.resource_id)
        .bind(request.policy_id)
        .bind(request.requested_by)
        .bind(request.status)
        .bind(request.current_step)
        .bind(request.priority)
        .bind(request.submitted_at)
        .bind(request.decided_at)
        .bind(request.decided_by)
        .bind(request.summary.clone())
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Insert one materialized step row.
    pub async fn insert_step(
        &self,
        conn: &mut sqlx::PgConnection,
        step: &ApprovalStep,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"INSERT INTO approvals.approval_steps
                   (id, company_id, request_id, step_no, approver_kind, approver_ref,
                    assigned_to, delegated_from, status, acted_at, comment, sla_due_at, metadata)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                       jsonb_build_object('created_by', to_jsonb($7::uuid), 'created_at', to_jsonb(now())))"#,
        )
        .bind(step.id)
        .bind(step.company_id)
        .bind(step.request_id)
        .bind(step.step_no)
        .bind(step.approver_kind)
        .bind(step.approver_ref)
        .bind(step.assigned_to)
        .bind(step.delegated_from)
        .bind(step.status)
        .bind(step.acted_at)
        .bind(&step.comment)
        .bind(step.sla_due_at)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Decide one member row. Row-truth: only a still-pending row matches; the service
    /// treats 0 rows as an already-decided idempotent no-op (it pre-filtered authorization).
    /// `delegated_from` records inherited authority when the decider acted via a live
    /// delegation window (COALESCE keeps any pre-resolved stamp).
    #[allow(clippy::too_many_arguments)]
    pub async fn mark_step_decided(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        step_id: Uuid,
        status: ApprovalStepStatus,
        comment: Option<&str>,
        delegated_from: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE approvals.approval_steps SET
                   status = $3,
                   acted_at = $6,
                   comment = COALESCE($4, comment),
                   delegated_from = COALESCE($5, delegated_from),
                   metadata = metadata || jsonb_build_object('updated_at', to_jsonb($6::timestamptz))
               WHERE company_id = $1 AND id = $2
                 AND status = 'pending'
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(company)
        .bind(step_id)
        .bind(status)
        .bind(comment)
        .bind(delegated_from)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Skip every other live pending step (`except`: the row that just decided, if any).
    /// The reject path and withdraw both funnel through here.
    pub async fn skip_other_pending_steps(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        request_id: Uuid,
        except: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"UPDATE approvals.approval_steps SET
                   status = 'skipped',
                   acted_at = $4,
                   metadata = metadata || jsonb_build_object('updated_at', to_jsonb($4::timestamptz))
               WHERE company_id = $1 AND request_id = $2
                 AND status = 'pending'
                 AND ($3::uuid IS NULL OR id <> $3)
                 AND (metadata->>'deleted_at') IS NULL"#,
        )
        .bind(company)
        .bind(request_id)
        .bind(except)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Advance the chain to the next step (all live members of the current one approved).
    /// None when a concurrent decider advanced/finished first — the caller re-reads.
    pub async fn set_current_step(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        request_id: Uuid,
        step_no: i32,
        now: DateTime<Utc>,
    ) -> Result<Option<ApprovalStatus>, sqlx::Error> {
        sqlx::query_scalar(
            r#"UPDATE approvals.approval_requests SET
                   current_step = $3,
                   metadata = metadata || jsonb_build_object('updated_at', to_jsonb($4::timestamptz))
               WHERE company_id = $1 AND id = $2
                 AND status = 'pending'
                  AND (metadata->>'deleted_at') IS NULL
               RETURNING status"#,
        )
        .bind(company)
        .bind(request_id)
        .bind(step_no)
        .bind(now)
        .fetch_optional(&mut *conn)
        .await
    }

    /// Terminal decision on the request (approved / rejected), stamped with the decider.
    /// None when a concurrent decider finished first — the caller re-reads.
    pub async fn finish_request(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        request_id: Uuid,
        status: ApprovalStatus,
        decided_by: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> Result<Option<ApprovalStatus>, sqlx::Error> {
        sqlx::query_scalar(
            r#"UPDATE approvals.approval_requests SET
                   status = $3,
                   decided_at = $5,
                   decided_by = $4,
                   metadata = metadata || jsonb_build_object('updated_at', to_jsonb($5::timestamptz))
               WHERE company_id = $1 AND id = $2
                 AND status = 'pending'
                  AND (metadata->>'deleted_at') IS NULL
               RETURNING status"#,
        )
        .bind(company)
        .bind(request_id)
        .bind(status)
        .bind(decided_by)
        .bind(now)
        .fetch_optional(&mut *conn)
        .await
    }

    /// Withdraw AND soft-delete the request: the stamp frees the per-resource partial
    /// unique so the consumer's re-submit files a fresh chain. None when the request
    /// stopped being pending concurrently — the caller re-reads.
    pub async fn withdraw_request(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        request_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Option<ApprovalStatus>, sqlx::Error> {
        sqlx::query_scalar(
            r#"UPDATE approvals.approval_requests SET
                   status = 'withdrawn',
                   decided_at = $3,
                   metadata = metadata || jsonb_build_object(
                       'deleted_at', to_jsonb($3::timestamptz),
                       'updated_at', to_jsonb($3::timestamptz))
               WHERE company_id = $1 AND id = $2
                 AND status = 'pending'
                  AND (metadata->>'deleted_at') IS NULL
               RETURNING status"#,
        )
        .bind(company)
        .bind(request_id)
        .bind(now)
        .fetch_optional(&mut *conn)
        .await
    }
}
