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
- **Dynamic approvers fail closed.** `manager_of` / `department_head_of` /
  `role_holder` / `position_holder` templates need an `ApproverResolver`; the
  shipped `FailClosedResolver` resolves none of them, so filing against such a
  template returns `422 step_resolution_failed`. Hosts that want dynamic
  resolution supply a resolver via `with_resolver` and own its semantics.
  `specific_employee` and `all_of` (of employees) resolve without a resolver.

## Deciding

- **Authorization is engine-side**, checked against the live rows at decide
  time — the HTTP layer vouches only for the tenant:
  1. `assigned_to == actor.employee_id`, or
  2. the actor holds a live delegation from that assignee (stamps
     `delegated_from` on the decided row), or
  3. the step's `approver_kind` is `role` and the actor's presented
     `role_refs` contain the step's `approver_ref`.
- **Reject fails fast.** One rejection decides the request `rejected` and
  skips every other pending step row — siblings never linger.
- **Approve counts down a quorum.** Each member row approves individually; the
  step (and only then the chain) advances when no live member row remains
  pending. Sequential templates gate naturally: step *n+1* rows exist from
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
  composing app's decision (typically an operator role).
- **One active policy per resource type** is the intended shape; if several
  are active the engine deterministically picks the earliest-created (then
  lowest id) rather than failing.
- **`role_refs` trust boundary.** Decide-time role authorization trusts the
  role ids the host's token vouches for. The host must derive them from its own
  role assignments — a client-supplied `roleRefs` is only as trustworthy as the
  HTTP layer that accepted it. The guarded routes document this but cannot
  enforce it; hosts behind stricter middleware can ignore the body field and
  inject verified refs.
- **Tenant binding.** Mount the guarded surface behind `company_auth` with the
  request-scoped DB binding (strict-RLS posture). Every engine statement rides
  a `bind_company_on` transaction with explicit `company_id` predicates; a
  cross-tenant id matches zero rows and surfaces as 404, never as leakage.
