# The approvals engine

`ApprovalsWriteService` (and its HTTP surface,
`create_guarded_approvals_routes`) is the decision engine behind the module's
CRUD scaffolding. Consumers file requests through their own seam adapters; the
engine owns every state change after that.

## Verbs

| Verb | Meaning |
|---|---|
| `file` | Link a resource to a chain (or a pre-approved verdict). Idempotent per resource. |
| `status` | Read the live verdict for a resource. |
| `decide` | Approve/reject one step as an authorized actor. |
| `withdraw` | Requester-only cancellation; frees the resource for a re-file. |

## Filing semantics

- **Idempotency first.** A live request row for `(company, resource_type,
  resource_id)` is returned as-is (`already_filed: true`). The concurrent loser
  of the partial-unique race catches `23505`, re-selects, and returns the
  existing request — retries always converge on the same request id.
- **No active policy → PRE-APPROVED, zero steps.** Tenants without a policy for
  the resource type behave exactly as they would without approvals wired: the
  verdict is immediate and positive. Policies are how a tenant opts into
  control, not a prerequisite for filing. A fail-closed default here would
  deadlock every policy-less consumer on the day it wires the seam.
- **Policy → every step materialized at file time.** One `approval_steps` row
  per template member, at the template's `step_no`. Delegation is resolved ONCE
  at this moment (a live window reassigns `assigned_to` to the delegate and
  stamps `delegated_from`); windows opened later do not re-resolve existing
  steps. An `all_of` template materializes one row per member at the same
  `step_no` — the step completes only when every live member approves.
- **Dynamic approvers fail closed.** `manager_of_requester` / `department_head` /
  `role` / `position` templates need an `ApproverResolver`; the
  shipped `FailClosedResolver` resolves none of them, so filing against such a
  template returns `422 step_resolution_failed`. Hosts that want dynamic
  resolution supply a resolver via `with_resolver` and own its semantics.
  `specific_employee` and `all_of` (of employees) resolve without a resolver.
  The single-approver kinds (`manager`, `department_head`) resolve to one
  employee; the multi-holder kinds (`role`, `position`) resolve to EVERY current
  holder and materialize one member row per holder, each stamped with the
  template's `approver_ref`. A dynamic kind that resolves to nobody leaves the
  step without a member and the whole filing refuses (`422
  invalid_approval_chain`).
- **Chains are validated at file time.** The engine walks step numbers from 1
  one at a time, so a chain whose distinct step numbers are not exactly
  `1..=max`, whose step holds no materialized member, or in which two members
  of one step resolve (after delegation) onto the same person would park the
  request on a step nobody can decide — every `decide` a 404 with withdraw the
  only exit. Filing rejects such chains with `422 invalid_approval_chain` and
  leaves no request row: an empty template set, a first step other than 1, a
  numbering gap, a duplicated quorum member, or two members sharing one
  delegate are all configuration faults the policy author fixes, not states
  the requester inherits. A step-insert unique violation that survives
  validation (a delegation edit racing the filing) surfaces as the same typed
  error, never a raw 500.
- **One active policy per resource.** The partial unique index
  `approval_policies_single_active` refuses a second active policy for a
  `(company, resource_type)` — a replacement policy must deactivate the old
  one first. Before the index, the engine's deterministic earliest-created
  pick made a replacement a silent no-op.

## Deciding

- **Authorization is engine-side**, checked against the live rows at decide
  time — the HTTP layer vouches only for the tenant, and nothing the client
  presented influences the outcome:
  1. `assigned_to == actor.employee_id`, or
  2. the actor holds a live delegation from that assignee (stamps
     `delegated_from` on the decided row).
- **Reject fails fast.** One rejection decides the request `rejected` and
  skips every other pending step row — siblings never linger.
- **Approve completes by regime.** Which rows a step needs depends on where
  they came from:

  | Step rows | Regime |
  |---|---|
  | `specific_employee`, `manager`, `department_head`, `all_of` members | **Every row** — a quorum; the step advances when no live member row remains pending. |
  | `role` / `position` template groups | **Any holder** — one holder's approval completes the group; the sibling rows are recorded `skipped` (the row answers why its holder never decided). A late sibling decide is the usual 409. |

  The skip is keyed on the full template identity (step number + kind +
  `approver_ref`). The partial unique index on `approval_step_templates
  (policy_id, step_no)` already keeps one live template per step number — a
  step is always a single template's regime, never a mix — so within one step
  the identity keying is exact by construction and stays exact as defense if
  rows from distinct origins ever share a number (soft-deleted template
  replaced at the same step, a relaxed index). Steps that must compose
  (quorum AND any-holder) are separate step numbers, decided in sequence.
- **Sequential templates gate naturally.** Step *n+1* rows exist from
  filing but the chain's `current_step` only reaches them after step *n*
  completes.
- **Concurrent deciders converge.** Advance/finish/withdraw row updates return
  `Option<ApprovalStatus>`; a `None` (someone else moved the chain first) makes
  the caller re-read the request instead of failing.

## Withdrawal

Requester-only. Marks the request `withdrawn` AND stamps `deleted_at` in
`metadata` — the soft-delete is what frees the per-resource partial unique, so
the consumer's re-submit files a fresh chain.

## What this module does NOT ship

- **No background sweeper.** A consumer row that stops pointing at its request
  (or vice versa) stays exactly as it is until someone withdraws or the request
  reaches a verdict. There is no reconciliation job; the position is that
  idempotent filing plus synchronous status reads keep the pair consistent, and
  a sweeper is additive later if a real drift case appears.
- **No escalation.** `sla_due_at` is stamped from the template but nothing acts
  on it. Escalation is a policy-semantics question (who, to whom, when) that
  belongs with a resolver-equipped host.
- **No outbox.** Consumers read verdicts synchronously over their seam port;
  there are no approval events to relay.

## Composition duties (host-owned)

- **Policy-admin RBAC.** The guarded surface exposes policy / step-template /
  delegation CRUD authenticated only by tenant. WHO may define chains is the
  composing app's decision (typically an operator role). Until the host has a
  role-checking middleware, mount the CRUD routers NOT AT ALL (seed operator
  master data in the database) — the rows are the authorization data the engine
  trusts at decide time, and any employee able to write them can name themselves
  approver or delegate themselves an approver's authority.
- **Approver resolution is host-owned.** The engine resolves `manager`,
  `department_head`, `role`, and `position` templates through the
  `ApproverResolver` the host wires via `with_resolver`; without one those
  kinds fail closed at file time. What "the manager" or "a role's holders"
  means is org data the host owns — the engine only trusts what the resolver
  returns, materialized once per filing.
- **Tenant binding.** Mount the guarded surface behind `company_auth` with the
  request-scoped DB binding (strict-RLS posture). Every engine statement rides
  a `bind_company_on` transaction with explicit `company_id` predicates; a
  cross-tenant id matches zero rows and surfaces as 404, never as leakage.
