//! Guarded route composition — the RECOMMENDED way to mount the approvals module.
//!
//! Hand-authored (user-owned; see `metaphor.codegen.yaml`). Splits the surface by actor:
//!
//! - **Guarded surface** ([`create_guarded_approvals_routes`]): engine verbs, the
//!   self-service delegation pair, and request/step reads — safe behind plain tenant
//!   auth, because every mutation's authorization is engine-side.
//! - **Request + step reads**: GETs only. No generic mutation reaches the engine's rows —
//!   the engine verbs own every state change.
//! - **Engine verbs**: `POST /approvals/requests/:id/decide`,
//!   `POST /approvals/requests/:id/withdraw`, and the self-service delegation pair
//!   `POST /approvals/delegations` + `POST /approvals/delegations/:id/revoke` over
//!   [`ApprovalsWriteService`], whose decide-time authorization is ENGINE-side
//!   (assigned approver / live delegation window) and whose delegating principal is
//!   always the token's `sub` — no client-supplied claim influences either.
//! - **Operator master data** ([`create_operator_master_data_routes`]): policy /
//!   step-template CRUD, kept OUT of the guarded surface on purpose — those rows are
//!   the authorization data the engine trusts, so they mount only behind a host's own
//!   RBAC gate (typically an operator role).
//!
//! The tenant comes from the [`CompanyContext`] the `company_auth` middleware inserts —
//! never from the body. Composers MUST mount this behind `company_auth` with the
//! request-scoped DB binding (the strict-RLS posture): a cross-tenant id matches zero rows
//! and surfaces as 404.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use backbone_auth::company::CompanyContext;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::application::service::approvals_write_service::{
    ApprovalsError, ApprovalsWriteService, ApproverActor, Decision,
};
use crate::domain::entity::ApprovalRequest;
use crate::ApprovalsModule;

use super::{
    create_approval_policy_routes, create_approval_request_read_routes,
    create_approval_step_read_routes, create_approval_step_template_routes,
};

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

fn err_response(e: ApprovalsError) -> axum::response::Response {
    let status = StatusCode::from_u16(e.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        Json(ErrorBody {
            error: e.code(),
            message: e.to_string(),
        }),
    )
        .into_response()
}

/// The HTTP shape of a request row — camelCase, verdict-first.
fn request_response(status: StatusCode, request: &ApprovalRequest) -> axum::response::Response {
    (status, Json(RequestBody::from(request))).into_response()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestBody {
    id: Uuid,
    company_id: Uuid,
    resource_type: String,
    resource_id: Uuid,
    policy_id: Option<Uuid>,
    requested_by: Uuid,
    status: String,
    current_step: Option<i32>,
    priority: String,
    submitted_at: Option<chrono::DateTime<chrono::Utc>>,
    decided_at: Option<chrono::DateTime<chrono::Utc>>,
    decided_by: Option<Uuid>,
    summary: Option<serde_json::Value>,
}

impl From<&ApprovalRequest> for RequestBody {
    fn from(r: &ApprovalRequest) -> Self {
        Self {
            id: r.id,
            company_id: r.company_id,
            resource_type: r.resource_type.to_string(),
            resource_id: r.resource_id,
            policy_id: r.policy_id,
            requested_by: r.requested_by,
            status: r.status.to_string(),
            current_step: r.current_step,
            priority: r.priority.to_string(),
            submitted_at: r.submitted_at,
            decided_at: r.decided_at,
            decided_by: r.decided_by,
            summary: r.summary.clone(),
        }
    }
}

/// The acting principal as a uuid actor stamp, when the token's `sub` parses as one.
fn actor(t: &CompanyContext) -> Option<Uuid> {
    Uuid::parse_str(&t.user_id).ok()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DecideBody {
    step_no: i32,
    /// `approve` or `reject`.
    decision: String,
    comment: Option<String>,
}

async fn decide(
    State(svc): State<Arc<ApprovalsWriteService>>,
    Path(request_id): Path<Uuid>,
    tenant: CompanyContext,
    Json(body): Json<DecideBody>,
) -> axum::response::Response {
    let Some(employee_id) = actor(&tenant) else {
        return err_response(ApprovalsError::NotStepApprover);
    };
    if body.decision != "approve" && body.decision != "reject" {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorBody {
                error: "invalid_decision",
                message: "decision must be \"approve\" or \"reject\"".into(),
            }),
        )
            .into_response();
    }
    let decision = Decision {
        company_id: tenant.company_id,
        request_id,
        step_no: body.step_no,
        actor: ApproverActor { employee_id },
        approve: body.decision == "approve",
        comment: body.comment,
    };
    match svc.decide(decision).await {
        Ok(status) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": status.to_string() })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn withdraw(
    State(svc): State<Arc<ApprovalsWriteService>>,
    Path(request_id): Path<Uuid>,
    tenant: CompanyContext,
) -> axum::response::Response {
    let Some(employee_id) = actor(&tenant) else {
        return err_response(ApprovalsError::NotRequester);
    };
    match svc
        .withdraw(tenant.company_id, request_id, employee_id)
        .await
    {
        Ok(status) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": status.to_string() })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn get_request(
    State(svc): State<Arc<ApprovalsWriteService>>,
    Path(request_id): Path<Uuid>,
    tenant: CompanyContext,
) -> axum::response::Response {
    match svc.get_request(tenant.company_id, request_id).await {
        Ok(r) => request_response(StatusCode::OK, &r),
        Err(e) => err_response(e),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDelegationBody {
    delegate_to: Uuid,
    valid_from: chrono::NaiveDate,
    valid_to: chrono::NaiveDate,
    reason: Option<String>,
}

/// Self-service delegation: the delegating approver is ALWAYS the token's `sub` —
/// a body `approverId`, if a client sends one, is ignored (unknown fields never
/// reach this handler). Delegation is consent; the principal cannot be forged.
async fn create_delegation(
    State(svc): State<Arc<ApprovalsWriteService>>,
    tenant: CompanyContext,
    Json(body): Json<CreateDelegationBody>,
) -> axum::response::Response {
    let Some(approver) = actor(&tenant) else {
        return err_response(ApprovalsError::NotDelegationApprover);
    };
    match svc
        .create_delegation(
            tenant.company_id,
            approver,
            body.delegate_to,
            body.valid_from,
            body.valid_to,
            body.reason.as_deref(),
        )
        .await
    {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"id": id, "status": "active"})),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn revoke_delegation(
    State(svc): State<Arc<ApprovalsWriteService>>,
    Path(delegation_id): Path<Uuid>,
    tenant: CompanyContext,
) -> axum::response::Response {
    let Some(approver) = actor(&tenant) else {
        return err_response(ApprovalsError::NotDelegationApprover);
    };
    match svc
        .revoke_delegation(tenant.company_id, delegation_id, approver)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(serde_json::json!({"status": "revoked"})),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

/// The engine verbs every mounting exposes: request read, decide, withdraw, and the
/// self-service delegation lifecycle. Factored so the module-held and host-supplied
/// composers cannot drift.
fn engine_verbs(svc: Arc<ApprovalsWriteService>) -> Router {
    Router::new()
        .route("/approvals/requests/:id", axum::routing::get(get_request))
        .route("/approvals/requests/:id/decide", post(decide))
        .route("/approvals/requests/:id/withdraw", post(withdraw))
        .route("/approvals/delegations", post(create_delegation))
        .route("/approvals/delegations/:id/revoke", post(revoke_delegation))
        .with_state(svc)
}

/// The guarded approvals surface — SAFE behind plain tenant auth (`company_auth`):
/// engine verbs, self-service delegation, and reads. It deliberately carries NO
/// operator master data: policy and step-template rows are the authorization data
/// the engine trusts at decide time, so their CRUD lives in
/// [`create_operator_master_data_routes`], which a host mounts behind its own RBAC
/// gate. `file` is also not here — consumers file through their own seam adapters.
pub fn create_guarded_approvals_routes(m: &ApprovalsModule) -> Router {
    Router::new()
        // Engine rows: reads only, plus the verbs.
        .merge(create_approval_request_read_routes(
            m.approval_request_service.clone(),
        ))
        .merge(create_approval_step_read_routes(
            m.approval_step_service.clone(),
        ))
        .merge(engine_verbs(m.approvals_write_service.clone()))
}

/// Operator master data: full policy / step-template CRUD. These rows ARE the
/// engine's authorization data — anyone who can write them can name themselves
/// approver on a policy the engine will trust. This composer authenticates ONLY the
/// tenant; a host MUST mount it behind its own role-checking middleware (an operator
/// role) and MUST NOT mount it bare. Until such a gate exists, leave this unmounted
/// and seed operator master data directly in the database.
pub fn create_operator_master_data_routes(m: &ApprovalsModule) -> Router {
    Router::new()
        .merge(create_approval_policy_routes(
            m.approval_policy_service.clone(),
        ))
        .merge(create_approval_step_template_routes(
            m.approval_step_template_service.clone(),
        ))
}

/// Convenience for hosts that build their own engine (e.g. with a resolver): the same
/// surface over a supplied service instead of the module-held one.
pub fn create_guarded_approvals_routes_with(svc: Arc<ApprovalsWriteService>) -> Router {
    engine_verbs(svc)
}
