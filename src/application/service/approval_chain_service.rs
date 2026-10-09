//! A policy's chain as one thing (hand-authored, user-owned; see `metaphor.codegen.yaml`).
//!
//! The generated CRUD writes step templates one row at a time. A chain edited that way
//! passes through broken states — a renumber needs temporary numbers (one live template
//! per `(policy_id, step_no)`), and a write that fails part-way leaves a gap or a hole.
//! `file` validates the whole chain, so a broken one refuses every new filing of that
//! kind until someone repairs the rows by hand. These verbs keep a chain whole:
//!
//! - **replace_chain** — the ordered steps, numbered 1..n, written in one transaction in
//!   place of the live ones (retired, not deleted, so their history stays). Requests
//!   already filed are untouched: their steps were materialized at filing.
//! - **preview_chain** — who each step of a chain (saved, or proposed) resolves to for a
//!   given requester, through the same resolver and delegation windows `file` uses, and
//!   which steps would resolve to nobody, said per step instead of as one refusal.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use backbone_orm::org_scope;

use crate::domain::entity::{ApprovalStepTemplate, ApproverKind};

use super::approvals_write_service::{ApprovalsError, ApprovalsWriteService};

/// The most steps one chain may hold; a longer one is almost certainly a mistake.
pub const MAX_CHAIN_STEPS: usize = 20;

/// One step of a chain, in order: who decides and how long they have.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainStep {
    pub approver_kind: ApproverKind,
    /// The employee, role or position the kind names; absent for the requester's manager or
    /// department head.
    #[serde(default)]
    pub approver_ref: Option<Uuid>,
    #[serde(default)]
    pub sla_hours: Option<i32>,
    /// An every-member quorum: these employees must all approve the step.
    #[serde(default)]
    pub all_of: Option<Vec<Uuid>>,
}

/// One step of a previewed chain: who would decide, or why nobody would.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepPreview {
    pub step_no: i32,
    pub approver_kind: ApproverKind,
    /// The employees the step would be assigned to, delegation applied.
    pub approvers: Vec<PreviewApprover>,
    /// Why the step would reach nobody (no manager, a department with no head, an empty
    /// role); a filing through this chain would be refused with it.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewApprover {
    pub employee_id: Uuid,
    /// Set when a live delegation window hands the step from this employee to `employee_id`.
    pub delegated_from: Option<Uuid>,
}

/// The shape every chain must have before anything is written or resolved.
pub fn check_chain(steps: &[ChainStep]) -> Result<(), ApprovalsError> {
    if steps.is_empty() {
        return Err(ApprovalsError::InvalidChain(
            "a chain needs at least one step".into(),
        ));
    }
    if steps.len() > MAX_CHAIN_STEPS {
        return Err(ApprovalsError::InvalidChain(format!(
            "a chain holds at most {MAX_CHAIN_STEPS} steps"
        )));
    }
    for (i, s) in steps.iter().enumerate() {
        let n = i + 1;
        if let Some(h) = s.sla_hours {
            if h <= 0 {
                return Err(ApprovalsError::InvalidChain(format!(
                    "step {n}: hours to decide must be more than zero"
                )));
            }
        }
        match &s.all_of {
            Some(ids) => {
                if ids.is_empty() {
                    return Err(ApprovalsError::InvalidQuorum);
                }
                let mut seen = std::collections::HashSet::new();
                if !ids.iter().all(|id| seen.insert(*id)) {
                    return Err(ApprovalsError::InvalidChain(format!(
                        "step {n}: the same person is named twice in its quorum"
                    )));
                }
            }
            None => {
                let needs_ref = matches!(
                    s.approver_kind,
                    ApproverKind::SpecificEmployee | ApproverKind::Role | ApproverKind::Position
                );
                if needs_ref && s.approver_ref.is_none() {
                    return Err(ApprovalsError::InvalidChain(format!(
                        "step {n}: name the person, role or position who decides"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn as_template(policy_id: Uuid, step_no: i32, s: &ChainStep) -> ApprovalStepTemplate {
    ApprovalStepTemplate {
        id: Uuid::new_v4(),
        policy_id,
        step_no,
        approver_kind: s.approver_kind,
        approver_ref: s.approver_ref,
        sla_hours: s.sla_hours,
        all_of: s.all_of.as_ref().map(|ids| {
            serde_json::Value::Array(ids.iter().map(|id| serde_json::json!(id)).collect())
        }),
        metadata: Default::default(),
    }
}

impl ApprovalsWriteService {
    /// Replace a policy's chain with these steps, numbered 1..n, in one transaction. The
    /// live templates are retired (soft-deleted) first, so the per-policy step numbers are
    /// free; either the whole new chain is written or nothing changes.
    pub async fn replace_chain(
        &self,
        policy_id: Uuid,
        steps: Vec<ChainStep>,
    ) -> Result<Vec<ApprovalStepTemplate>, ApprovalsError> {
        check_chain(&steps)?;
        let mut tx = self.rpool().begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        if !self.repo.policy_exists(&mut tx, policy_id).await? {
            return Err(ApprovalsError::InvalidChain(
                "no such approval policy".into(),
            ));
        }
        self.repo.retire_templates(&mut tx, policy_id).await?;
        let mut written = Vec::with_capacity(steps.len());
        for (i, s) in steps.iter().enumerate() {
            let t = as_template(policy_id, i as i32 + 1, s);
            written.push(self.repo.insert_template(&mut tx, &t).await?);
        }
        tx.commit().await?;
        Ok(written)
    }

    /// Who each step of a chain resolves to for `requester`: the proposed `steps` when
    /// given, else the policy's saved chain. Read-only — the transaction is rolled back.
    pub async fn preview_chain(
        &self,
        policy_id: Uuid,
        steps: Option<Vec<ChainStep>>,
        requester: Uuid,
    ) -> Result<Vec<StepPreview>, ApprovalsError> {
        let mut tx = self.rpool().begin().await?;
        if let Some(scope) = org_scope::current_org_scope() {
            org_scope::bind_org_scope_on(&mut tx, &scope).await?;
        }
        let templates = match steps {
            Some(steps) => {
                check_chain(&steps)?;
                steps
                    .iter()
                    .enumerate()
                    .map(|(i, s)| as_template(policy_id, i as i32 + 1, s))
                    .collect::<Vec<_>>()
            }
            None => {
                if !self.repo.policy_exists(&mut tx, policy_id).await? {
                    return Err(ApprovalsError::InvalidChain(
                        "no such approval policy".into(),
                    ));
                }
                self.repo.templates_for_policy(&mut tx, policy_id).await?
            }
        };

        let mut out = Vec::with_capacity(templates.len());
        for t in &templates {
            let (approvers, problem) = match self.resolve_members(t, requester).await {
                Ok(members) if members.is_empty() => {
                    (Vec::new(), Some("resolves to no approver".to_string()))
                }
                Ok(members) => {
                    let mut list = Vec::with_capacity(members.len());
                    for m in members {
                        let (to, from) = self.apply_delegation(&mut tx, m.assigned_to).await?;
                        list.push(PreviewApprover {
                            employee_id: to,
                            delegated_from: from,
                        });
                    }
                    let mut seen = std::collections::HashSet::new();
                    let twice = !list.iter().all(|a| seen.insert(a.employee_id));
                    let problem = if list.iter().any(|a| a.employee_id == requester) {
                        Some("the requester would decide their own request".to_string())
                    } else if twice {
                        Some("two members resolve to the same approver".to_string())
                    } else {
                        None
                    };
                    (list, problem)
                }
                Err(e) => (Vec::new(), Some(e.to_string())),
            };
            out.push(StepPreview {
                step_no: t.step_no,
                approver_kind: t.approver_kind,
                approvers,
                problem,
            });
        }
        tx.rollback().await?;
        Ok(out)
    }
}
