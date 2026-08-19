//! `ApprovalsWriteService` — the decision engine (hand-authored, user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! The generated CRUD surface only moves rows; THIS service owns the semantics:
//!
//! - **file** — idempotent per resource: one LIVE request per `(company, resource_type,
//!   resource_id)` (partial unique). A tenant with NO active policy for the resource gets a
//!   PRE-APPROVED request with zero steps — control is opt-in via policy, so a policy-less
//!   tenant behaves exactly as it did before the engine existed. A policy materializes the
//!   chain: one step row per template member at its `step_no` (an all-of quorum is one row
//!   per member at the SAME step_no — the quorum unique index), approvers resolved at file
//!   time, any live delegation window pre-applied (`assigned_to` = delegate,
//!   `delegated_from` = original approver).
//! - **decide** — engine-side authorization: the actor must BE the assigned approver or
//!   hold an active delegation from them. A reject fails fast: the request is rejected and
//!   every other live pending step is skipped. An approve marks the member row; a step
//!   completes when ALL its live members approved, then the chain advances (or finishes
//!   `approved`).
//! - **status** — the raw engine verdict; consumer adapters translate into their own Verdict
//!   enums at the seam.
//! - **withdraw** — requester-only; a pending request is withdrawn AND soft-deleted, which
//!   frees the per-resource unique so a re-submit files a fresh chain.
//!
//! There is NO background sweeper: a filing orphaned by a crashed or raced consumer stays
//! `pending` until its requester withdraws it; retries converge because `file` is
//! idempotent per resource. `sla_due_at` is stamped but escalation is deferred.
//!
//! RLS discipline (ADR-0008/0014): every statement runs inside a `bind_company_on`
//! transaction on the caller's connection and additionally carries its own `company_id`
//! predicate — a cross-tenant id matches zero rows (404), never leakage.

use std::sync::Arc;

use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_orm::company_scope;

use crate::domain::entity::{
    ApprovalPriority, ApprovalRequest, ApprovalResourceType, ApprovalStatus, ApprovalStep,
    ApprovalStepStatus, ApproverKind,
};
use crate::infrastructure::persistence::ApprovalsWriteRepository;

/// A filing a consumer's seam hands the engine.
#[derive(Debug, Clone)]
pub struct FileFiling {
    pub company_id: Uuid,
    pub resource_type: ApprovalResourceType,
    pub resource_id: Uuid,
    pub requested_by: Uuid,
    pub priority: ApprovalPriority,
    /// Payload snapshot shown to the approver (the consumer's own shape).
    pub summary: serde_json::Value,
}

/// What `file` did.
#[derive(Debug, Clone)]
pub struct FilingOutcome {
    pub request_id: Uuid,
    /// `pending` under a policy; `approved` for the no-policy pre-approved posture.
    pub verdict: ApprovalStatus,
    /// true when a live request already existed and was returned as-is.
    pub already_filed: bool,
}

/// The deciding principal, as the HOST vouches for them. Authorization is checked
/// engine-side against the materialized rows (assigned approver or a live delegation
/// window); no client-supplied claim influences it.
#[derive(Debug, Clone)]
pub struct ApproverActor {
    pub employee_id: Uuid,
}

/// One decision on one step.
#[derive(Debug, Clone)]
pub struct Decision {
    pub company_id: Uuid,
    pub request_id: Uuid,
    pub step_no: i32,
    pub actor: ApproverActor,
    pub approve: bool,
    pub comment: Option<String>,
}

/// Resolves the dynamic approver kinds a policy template may name.
///
/// `manager_of` / `department_head_of` are single approvers by shape (one active
/// employment → one line manager; one department → one head) — when a host genuinely has
/// co-heads, `Err` is the fail-closed answer: the filing refuses typed rather than the
/// engine silently picking. `role_holders` / `position_holders` return EVERY current
/// holder — the engine materializes one member row per holder and the step is
/// any-holder (the first approval completes it; see `decide`).
///
/// The engine ships only [`FailClosedResolver`]: without a host-supplied resolver, any
/// policy naming a dynamic kind fails the filing closed (422 `step_resolution_failed`)
/// rather than guessing.
#[async_trait::async_trait]
pub trait ApproverResolver: Send + Sync {
    async fn manager_of(&self, company: Uuid, requester: Uuid) -> Result<Uuid, String>;
    async fn department_head_of(&self, company: Uuid, requester: Uuid) -> Result<Uuid, String>;
    async fn role_holders(&self, company: Uuid, role: Uuid) -> Result<Vec<Uuid>, String>;
    async fn position_holders(&self, company: Uuid, position: Uuid) -> Result<Vec<Uuid>, String>;
}

/// The shipped resolver: every dynamic kind is a hard error. Wire a real one via
/// [`ApprovalsWriteService::with_resolver`] when the host can resolve org structure.
pub struct FailClosedResolver;

#[async_trait::async_trait]
impl ApproverResolver for FailClosedResolver {
    async fn manager_of(&self, _company: Uuid, _requester: Uuid) -> Result<Uuid, String> {
        Err("manager_of_requester resolution requires a host-supplied ApproverResolver".into())
    }
    async fn department_head_of(&self, _company: Uuid, _requester: Uuid) -> Result<Uuid, String> {
        Err("department_head resolution requires a host-supplied ApproverResolver".into())
    }
    async fn role_holders(&self, _company: Uuid, _role: Uuid) -> Result<Vec<Uuid>, String> {
        Err("role resolution requires a host-supplied ApproverResolver".into())
    }
    async fn position_holders(&self, _company: Uuid, _position: Uuid) -> Result<Vec<Uuid>, String> {
        Err("position resolution requires a host-supplied ApproverResolver".into())
    }
}

/// Typed engine errors — stable `code()` strings for the HTTP surface and consumer seams.
#[derive(Debug, thiserror::Error)]
pub enum ApprovalsError {
    #[error("approval request not found")]
    NotFound,
    #[error("the request is not pending a decision")]
    NotPending,
    #[error("the chain is not waiting on this step yet")]
    OutOfTurn,
    #[error("the request has no such step")]
    NoSuchStep,
    #[error("this actor may not decide this step")]
    NotStepApprover,
    #[error("only the requester may withdraw")]
    NotRequester,
    #[error("approver resolution failed: {0}")]
    StepResolutionFailed(String),
    #[error("the template's all_of quorum must be an array of employee ids")]
    InvalidQuorum,
    #[error("the policy's approval chain is invalid: {0}")]
    InvalidChain(String),
    #[error("the template names a specific approver without a reference")]
    MissingApproverRef,
    #[error("filing lost the concurrent-filing race too many times — retry")]
    FilingRaceExhausted,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl ApprovalsError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "approval_request_not_found",
            Self::NotPending => "not_pending",
            Self::OutOfTurn => "out_of_turn",
            Self::NoSuchStep => "no_such_step",
            Self::NotStepApprover => "not_step_approver",
            Self::NotRequester => "not_requester",
            Self::StepResolutionFailed(_) => "step_resolution_failed",
            Self::InvalidQuorum => "invalid_quorum",
            Self::InvalidChain(_) => "invalid_approval_chain",
            Self::MissingApproverRef => "missing_approver_ref",
            Self::FilingRaceExhausted => "filing_race_exhausted",
            Self::Db(_) => "database_error",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::NotFound | Self::NoSuchStep => 404,
            Self::NotStepApprover | Self::NotRequester => 403,
            Self::NotPending | Self::OutOfTurn => 409,
            Self::StepResolutionFailed(_)
            | Self::InvalidQuorum
            | Self::MissingApproverRef
            | Self::InvalidChain(_) => 422,
            Self::FilingRaceExhausted => 503,
            Self::Db(_) => 500,
        }
    }
}

/// How many laps `file` takes before giving up on the concurrent-filing race.
const FILE_ATTEMPTS: usize = 5;

/// One materialized member of a step (before delegation is applied).
struct ResolvedMember {
    approver_kind: ApproverKind,
    approver_ref: Option<Uuid>,
    assigned_to: Uuid,
}

pub struct ApprovalsWriteService {
    pool: PgPool,
    repo: ApprovalsWriteRepository,
    resolver: Arc<dyn ApproverResolver>,
}

impl ApprovalsWriteService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            repo: ApprovalsWriteRepository,
            resolver: Arc::new(FailClosedResolver),
        }
    }

    /// Supply a resolver for the dynamic approver kinds (manager / department head / role /
    /// position). Without one, policies naming those kinds fail closed at file time.
    pub fn with_resolver(mut self, resolver: Arc<dyn ApproverResolver>) -> Self {
        self.resolver = resolver;
        self
    }

    /// File (or return the already-live) approval request for one resource. Idempotent;
    /// the concurrent-filing loser of the partial-unique race re-selects the winner's row.
    pub async fn file(&self, filing: FileFiling) -> Result<FilingOutcome, ApprovalsError> {
        let now = Utc::now();

        // Concurrent filings of one resource converge on one request. The partial-unique
        // loser cannot re-select inside its own transaction (PostgreSQL aborts it on the
        // error), so it rolls back and takes another lap: the next iteration either reads
        // the winner's row, or — if the winner's row was withdrawn in the same instant —
        // files a fresh one. Both outcomes are correct.
        for _ in 0..FILE_ATTEMPTS {
            let mut tx = self.pool.begin().await?;
            company_scope::bind_company_on(&mut tx, filing.company_id).await?;

            if let Some(existing) = self
                .repo
                .find_live_request(
                    &mut tx,
                    filing.company_id,
                    &filing.resource_type,
                    filing.resource_id,
                )
                .await?
            {
                tx.commit().await?;
                return Ok(FilingOutcome {
                    request_id: existing.id,
                    verdict: existing.status,
                    already_filed: true,
                });
            }

            // The active policy for this resource, if the tenant opted into control.
            // Deterministic pick when several are active (see docs/approvals-engine.md —
            // enforcing a single active policy per resource is a host duty for now).
            let policy = self
                .repo
                .find_active_policy(&mut tx, filing.company_id, &filing.resource_type)
                .await?;

            let request = ApprovalRequest {
                id: Uuid::new_v4(),
                company_id: filing.company_id,
                resource_type: filing.resource_type,
                resource_id: filing.resource_id,
                policy_id: policy.as_ref().map(|p| p.id),
                requested_by: filing.requested_by,
                status: if policy.is_some() {
                    ApprovalStatus::Pending
                } else {
                    ApprovalStatus::Approved
                },
                current_step: policy.as_ref().map(|_| 1),
                priority: filing.priority,
                submitted_at: Some(now),
                decided_at: if policy.is_none() { Some(now) } else { None },
                decided_by: None,
                summary: Some(filing.summary.clone()),
                metadata: Default::default(),
            };

            match self.repo.insert_request(&mut tx, &request).await {
                Ok(()) => {}
                Err(e) if is_unique_violation(&e) => {
                    // The transaction is aborted — roll it back before anything else.
                    tx.rollback().await?;
                    continue;
                }
                Err(e) => return Err(e.into()),
            }

            if let Some(policy) = policy {
                let templates = self.repo.templates_for_policy(&mut tx, policy.id).await?;
                // Resolve the whole chain first, then validate it as a whole, then insert.
                // The engine walks step numbers one at a time from 1, so a chain that is
                // empty, starts above 1, skips a number, or resolves two members of one
                // step onto the same approver would park the request on a step nobody can
                // decide — every decision a 404 and withdraw the only exit. Rejecting at
                // file time keeps the misconfiguration recoverable by editing the policy.
                let mut rows: Vec<ApprovalStep> = Vec::new();
                for template in &templates {
                    let members = self.resolve_members(template, filing.requested_by).await?;
                    for member in members {
                        let (assigned_to, delegated_from) = self
                            .apply_delegation(&mut tx, filing.company_id, member.assigned_to)
                            .await?;
                        rows.push(ApprovalStep {
                            id: Uuid::new_v4(),
                            company_id: filing.company_id,
                            request_id: request.id,
                            step_no: template.step_no,
                            approver_kind: member.approver_kind,
                            approver_ref: member.approver_ref,
                            assigned_to,
                            delegated_from,
                            status: ApprovalStepStatus::Pending,
                            acted_at: None,
                            comment: None,
                            sla_due_at: template
                                .sla_hours
                                .map(|h| now + chrono::Duration::hours(h as i64)),
                            metadata: Default::default(),
                        });
                    }
                }
                Self::validate_chain(&templates, &rows)?;
                for step in rows {
                    match self.repo.insert_step(&mut tx, &step).await {
                        Ok(()) => {}
                        // Distinctness was just validated; this is reachable only when a
                        // delegation edit races the filing onto the same approver — a
                        // chain fault, not a raw 500.
                        Err(e) if is_unique_violation(&e) => {
                            return Err(ApprovalsError::InvalidChain(
                                "two members of one step resolve to the same approver"
                                    .to_string(),
                            ))
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            }

            tx.commit().await?;
            return Ok(FilingOutcome {
                request_id: request.id,
                verdict: request.status,
                already_filed: false,
            });
        }

        Err(ApprovalsError::FilingRaceExhausted)
    }

    /// Resolve one template into its concrete member rows (a quorum template yields many).
    async fn resolve_members(
        &self,
        template: &crate::domain::entity::ApprovalStepTemplate,
        requester: Uuid,
    ) -> Result<Vec<ResolvedMember>, ApprovalsError> {
        // all-of quorum: the template lists the members; each becomes a row at this step_no
        // and the step completes only when every row is approved.
        if let Some(all_of) = &template.all_of {
            let Some(ids) = all_of.as_array() else {
                return Err(ApprovalsError::InvalidQuorum);
            };
            let mut members = Vec::with_capacity(ids.len());
            for id in ids {
                let Ok(employee) = serde_json::from_value::<Uuid>(id.clone()) else {
                    return Err(ApprovalsError::InvalidQuorum);
                };
                members.push(ResolvedMember {
                    approver_kind: ApproverKind::SpecificEmployee,
                    approver_ref: Some(employee),
                    assigned_to: employee,
                });
            }
            if members.is_empty() {
                return Err(ApprovalsError::InvalidQuorum);
            }
            return Ok(members);
        }

        let one = move |kind: ApproverKind, r: Option<Uuid>, to: Uuid| {
            vec![ResolvedMember {
                approver_kind: kind,
                approver_ref: r,
                assigned_to: to,
            }]
        };
        match template.approver_kind {
            ApproverKind::SpecificEmployee => match template.approver_ref {
                Some(employee) => Ok(one(
                    ApproverKind::SpecificEmployee,
                    Some(employee),
                    employee,
                )),
                None => Err(ApprovalsError::MissingApproverRef),
            },
            ApproverKind::ManagerOfRequester => {
                let manager = self
                    .resolver
                    .manager_of(template.company_id, requester)
                    .await
                    .map_err(ApprovalsError::StepResolutionFailed)?;
                Ok(one(ApproverKind::ManagerOfRequester, None, manager))
            }
            ApproverKind::DepartmentHead => {
                let head = self
                    .resolver
                    .department_head_of(template.company_id, requester)
                    .await
                    .map_err(ApprovalsError::StepResolutionFailed)?;
                Ok(one(ApproverKind::DepartmentHead, None, head))
            }
            ApproverKind::Role => {
                let role = template
                    .approver_ref
                    .ok_or(ApprovalsError::MissingApproverRef)?;
                let holders = self
                    .resolver
                    .role_holders(template.company_id, role)
                    .await
                    .map_err(ApprovalsError::StepResolutionFailed)?;
                // Every holder becomes a member row, each stamped with the template's
                // role ref (the template identity decide-time semantics key on). Zero
                // holders leaves the step without a member — validate_chain refuses.
                Ok(holders
                    .into_iter()
                    .map(|holder| ResolvedMember {
                        approver_kind: ApproverKind::Role,
                        approver_ref: Some(role),
                        assigned_to: holder,
                    })
                    .collect())
            }
            ApproverKind::Position => {
                let position = template
                    .approver_ref
                    .ok_or(ApprovalsError::MissingApproverRef)?;
                let holders = self
                    .resolver
                    .position_holders(template.company_id, position)
                    .await
                    .map_err(ApprovalsError::StepResolutionFailed)?;
                Ok(holders
                    .into_iter()
                    .map(|holder| ResolvedMember {
                        approver_kind: ApproverKind::Position,
                        approver_ref: Some(position),
                        assigned_to: holder,
                    })
                    .collect())
            }
        }
    }

    /// A chain is walkable only when its distinct step numbers are exactly 1..=max, every
    /// step holds at least one materialized member, and no approver appears twice within
    /// one step once delegations are applied. Everything else parks the request on a step
    /// no live row covers. Several templates MAY share a step number (an and-composition
    /// at that step); the walk only cares about the distinct numbers.
    fn validate_chain(
        templates: &[crate::domain::entity::ApprovalStepTemplate],
        rows: &[ApprovalStep],
    ) -> Result<(), ApprovalsError> {
        if templates.is_empty() {
            return Err(ApprovalsError::InvalidChain(
                "the active policy has no step templates".to_string(),
            ));
        }
        let mut steps: Vec<i32> = templates.iter().map(|t| t.step_no).collect();
        steps.sort_unstable();
        steps.dedup();
        if steps[0] != 1 {
            return Err(ApprovalsError::InvalidChain(format!(
                "step numbers must start at 1 (found {} first)",
                steps[0]
            )));
        }
        for (i, s) in steps.iter().enumerate() {
            if *s != i as i32 + 1 {
                return Err(ApprovalsError::InvalidChain(format!(
                    "step numbers must be contiguous (missing step {})",
                    i + 1
                )));
            }
        }
        for &s in &steps {
            let mut seen = std::collections::HashSet::new();
            let mut members_at_step = 0usize;
            for m in rows.iter().filter(|r| r.step_no == s) {
                members_at_step += 1;
                if !seen.insert(m.assigned_to) {
                    return Err(ApprovalsError::InvalidChain(format!(
                        "step {s} resolves two members onto the same approver \
                         (a duplicated member or a shared delegate)"
                    )));
                }
            }
            if members_at_step == 0 {
                return Err(ApprovalsError::InvalidChain(format!(
                    "step {s} resolves to no approver"
                )));
            }
        }
        Ok(())
    }

    /// Pre-apply a live delegation window: the delegate decides, the row records whose
    /// authority it inherited. Resolution happens once, at file time — a delegation created
    /// AFTER the chain materialized does not re-route existing step rows (see
    /// docs/approvals-engine.md).
    async fn apply_delegation(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        approver: Uuid,
    ) -> Result<(Uuid, Option<Uuid>), ApprovalsError> {
        let delegation = self
            .repo
            .find_active_delegation_for(conn, company, approver)
            .await?;
        Ok(match delegation {
            Some(delegate) => (delegate, Some(approver)),
            None => (approver, None),
        })
    }

    /// Decide the current step. Authorization is engine-side; the reject path fails fast.
    pub async fn decide(&self, decision: Decision) -> Result<ApprovalStatus, ApprovalsError> {
        let now = Utc::now();
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, decision.company_id).await?;

        let request = self
            .repo
            .get_request(&mut tx, decision.company_id, decision.request_id)
            .await?
            .ok_or(ApprovalsError::NotFound)?;
        if request.status != ApprovalStatus::Pending {
            return Err(ApprovalsError::NotPending);
        }
        if request.current_step != Some(decision.step_no) {
            return Err(ApprovalsError::OutOfTurn);
        }

        let steps = self
            .repo
            .live_steps_at(
                &mut tx,
                decision.company_id,
                decision.request_id,
                decision.step_no,
            )
            .await?;
        if steps.is_empty() {
            return Err(ApprovalsError::NoSuchStep);
        }

        // Engine-side authorization per row: the assigned approver or a live delegation
        // FROM them — nothing the client presented influences this.
        let mut authorized: Vec<&ApprovalStep> = Vec::new();
        for step in &steps {
            if self
                .actor_may_decide(&mut tx, decision.company_id, step, &decision.actor)
                .await?
            {
                authorized.push(step);
            }
        }
        let Some(step) = authorized
            .iter()
            .find(|s| s.status == ApprovalStepStatus::Pending)
            .copied()
        else {
            if authorized.is_empty() {
                return Err(ApprovalsError::NotStepApprover);
            }
            // The actor already approved their member row — idempotent no-op.
            tx.commit().await?;
            return Ok(request.status);
        };

        // An actor deciding someone else's row did so via a live delegation window
        // (authorization is exactly assignee-or-delegation) — stamp the authority source,
        // matching the file-time pre-resolution stamp (`delegated_from` is always WHOSE
        // authority the decider used, never the decider).
        let inherited_from = if step.assigned_to == decision.actor.employee_id {
            None
        } else {
            Some(step.assigned_to)
        };

        if !decision.approve {
            // Fail fast: this member row rejects, the request rejects, every other live
            // pending step is skipped.
            self.repo
                .mark_step_decided(
                    &mut tx,
                    decision.company_id,
                    step.id,
                    ApprovalStepStatus::Rejected,
                    decision.comment.as_deref(),
                    inherited_from,
                    now,
                )
                .await?;
            self.repo
                .skip_other_pending_steps(
                    &mut tx,
                    decision.company_id,
                    decision.request_id,
                    Some(step.id),
                    now,
                )
                .await?;
            let final_status = match self
                .repo
                .finish_request(
                    &mut tx,
                    decision.company_id,
                    decision.request_id,
                    ApprovalStatus::Rejected,
                    Some(decision.actor.employee_id),
                    now,
                )
                .await?
            {
                Some(status) => status,
                // A concurrent decider finished first — report how it landed.
                None => self
                    .repo
                    .get_request(&mut tx, decision.company_id, decision.request_id)
                    .await?
                    .map(|r| r.status)
                    .unwrap_or(ApprovalStatus::Rejected),
            };
            tx.commit().await?;
            return Ok(final_status);
        }

        self.repo
            .mark_step_decided(
                &mut tx,
                decision.company_id,
                step.id,
                ApprovalStepStatus::Approved,
                decision.comment.as_deref(),
                inherited_from,
                now,
            )
            .await?;

        // The step completes when every live member row is approved.
        let remaining = self
            .repo
            .count_pending_steps_at(
                &mut tx,
                decision.company_id,
                decision.request_id,
                decision.step_no,
            )
            .await?;
        let status = if remaining == 0 {
            // Last step? finish approved; otherwise advance the chain.
            let max_step = self
                .repo
                .max_step_no(&mut tx, decision.company_id, decision.request_id)
                .await?;
            let landed = if decision.step_no >= max_step {
                self.repo
                    .finish_request(
                        &mut tx,
                        decision.company_id,
                        decision.request_id,
                        ApprovalStatus::Approved,
                        Some(decision.actor.employee_id),
                        now,
                    )
                    .await?
            } else {
                self.repo
                    .set_current_step(
                        &mut tx,
                        decision.company_id,
                        decision.request_id,
                        decision.step_no + 1,
                        now,
                    )
                    .await?
            };
            // A concurrent decider advanced/finished first — either way the row this actor
            // approved is recorded; report the converged status.
            landed.unwrap_or(request.status)
        } else {
            request.status
        };

        tx.commit().await?;
        Ok(status)
    }

    async fn actor_may_decide(
        &self,
        conn: &mut sqlx::PgConnection,
        company: Uuid,
        step: &ApprovalStep,
        actor: &ApproverActor,
    ) -> Result<bool, ApprovalsError> {
        if step.assigned_to == actor.employee_id {
            return Ok(true);
        }
        // The actor holds a live delegation FROM the assigned approver → they decide,
        // and the row records the inherited authority.
        if self
            .repo
            .delegation_exists_for(conn, company, step.assigned_to, actor.employee_id)
            .await?
        {
            return Ok(true);
        }
        Ok(false)
    }

    /// The engine verdict for one request (consumer seams translate this into their own
    /// Verdict enums). Cross-tenant ids are 404s, never leakage.
    pub async fn status(
        &self,
        company: Uuid,
        request_id: Uuid,
    ) -> Result<ApprovalStatus, ApprovalsError> {
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company).await?;
        let request = self
            .repo
            .get_request(&mut tx, company, request_id)
            .await?
            .ok_or(ApprovalsError::NotFound)?;
        tx.commit().await?;
        Ok(request.status)
    }

    /// The full request row (the guarded read surface uses this).
    pub async fn get_request(
        &self,
        company: Uuid,
        request_id: Uuid,
    ) -> Result<ApprovalRequest, ApprovalsError> {
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company).await?;
        let request = self
            .repo
            .get_request(&mut tx, company, request_id)
            .await?
            .ok_or(ApprovalsError::NotFound)?;
        tx.commit().await?;
        Ok(request)
    }

    /// Requester-only withdraw: the pending chain is withdrawn AND soft-deleted, freeing the
    /// per-resource unique so a re-submit files a fresh chain.
    pub async fn withdraw(
        &self,
        company: Uuid,
        request_id: Uuid,
        actor: Uuid,
    ) -> Result<ApprovalStatus, ApprovalsError> {
        let now = Utc::now();
        let mut tx = self.pool.begin().await?;
        company_scope::bind_company_on(&mut tx, company).await?;

        let request = self
            .repo
            .get_request(&mut tx, company, request_id)
            .await?
            .ok_or(ApprovalsError::NotFound)?;
        if request.requested_by != actor {
            return Err(ApprovalsError::NotRequester);
        }
        if request.status != ApprovalStatus::Pending {
            return Err(ApprovalsError::NotPending);
        }

        self.repo
            .skip_other_pending_steps(&mut tx, company, request_id, None, now)
            .await?;
        let status = self
            .repo
            .withdraw_request(&mut tx, company, request_id, now)
            .await?
            .unwrap_or(ApprovalStatus::Withdrawn);
        tx.commit().await?;
        Ok(status)
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .map(|d| d.code().as_deref() == Some("23505"))
        .unwrap_or(false)
}
