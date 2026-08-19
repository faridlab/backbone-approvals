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
| `delegate` | Self-service: an approver grants a delegate their authority for a window. |
| `revoke` | Self-service: the delegating approver ends that window. |

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

## Delegation is self-service

Delegation rows carry real authority (a live window lets the delegate decide the
approver's steps), so the guarded surface exposes them ONLY as principal-verbs:
`POST /approvals/delegations` and `POST /approvals/delegations/:id/revoke`.
The delegating approver is always the token's `sub` — a body `approverId` is
ignored, which makes consent structural: nobody can author a window on another
approver's behalf. The generic delegation CRUD stays unmounted from the guarded
composer for the same reason.

- **Create** refuses self-delegation (`422 self_delegation_refused`) and an
  inverted window (`422 delegation_window_invalid`); the row inserts `active`
  with `source: self_service` in its audit metadata.
- **Revoke** is approver-only (`403 not_delegation_approver`), row-truth on the
  still-active predicate: a concurrent or repeated revoke is a typed
  `409 delegation_not_active`, never a silent second success.
- **Revoke does not re-route materialized rows.** Delegation resolves ONCE at
  file time (it reassigns then-materialized steps and stamps `delegated_from`);
  revoking later stops decide-time authorization and future filings but leaves
  rows a filing already resolved in place — including their assignee: after a
  revoke, the DELEGATE still decides a row the window routed to them, while the
  approver no longer can (the row's `assigned_to` is the delegate; the engine
  checks delegation from the assignee, not the original approver). Pulling back
  an in-flight chain is the requester's withdraw-and-refile, not a delegation
  edit — and it only works while the request is still pending.

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

- **Policy-admin RBAC.** Policy / step-template CRUD (the
  [`create_operator_master_data_routes`] composer, or the per-entity
  `*_write_routes` composers it is built from) authenticates only the tenant.
  WHO may define chains is the composing app's decision (typically an operator
  role). Until the host has a role-checking middleware, mount the CRUD routers
  NOT AT ALL (seed operator master data in the database) — the rows are the
  authorization data the engine trusts at decide time, and any employee able to
  write them can name themselves approver or delegate themselves an approver's
  authority.
- **Never mount the generated generic routers.** The schema generator also
  emits per-entity route modules and aggregate composers (`routes::mod`
  `create_stateless_routes`, `all_crud_routes`, `routes()`), which mount
  tenant-fenced-only CRUD on EVERY entity — including delegations (undoing the
  self-service consent model) and the engine's own request/step rows (writing
  verdicts around `decide`). Those files are generator-owned and cannot carry
  this warning themselves. The only production-safe surfaces are
  [`create_guarded_approvals_routes`] (behind `company_auth`) plus
  [`create_operator_master_data_routes`] (behind the host's RBAC gate);
  `readonly_routes()` is safe as a read base.
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
