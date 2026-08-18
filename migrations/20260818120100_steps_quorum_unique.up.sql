-- Quorum-capable step uniqueness.
--
-- A step whose template carries an all-of quorum materializes ONE row per member at the
-- SAME (request_id, step_no) — the members differ only by assigned_to. The original
-- unique index on (request_id, step_no) forbade that shape, so it is replaced by
-- (request_id, step_no, assigned_to): still one live row per approver per step, but a
-- quorum's members coexist. Within a non-quorum step the pair remains unique in practice
-- (only one row is ever written for it).
--
-- Hand-written: the schema DSL expresses the new columns but not the "replace an existing
-- partial unique" intent, so the schema YAML and this migration are kept in lockstep
-- manually (schema/models/approval_step.model.yaml carries the same column list).

DROP INDEX IF EXISTS approvals.idx_approval_steps_request_id_step_no;

CREATE UNIQUE INDEX IF NOT EXISTS idx_approval_steps_request_id_step_no_assigned_to
    ON approvals.approval_steps (request_id, step_no, assigned_to)
    WHERE (metadata->>'deleted_at') IS NULL;
