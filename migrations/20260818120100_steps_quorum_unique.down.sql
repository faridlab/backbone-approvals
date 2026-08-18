-- Restore the pre-quorum uniqueness: one live row per (request_id, step_no).
-- Only safe when no quorum rows exist (multiple assigned_to at one step_no); the
-- creating index is dropped first so the restore is a plain swap back.

DROP INDEX IF EXISTS approvals.idx_approval_steps_request_id_step_no_assigned_to;

CREATE UNIQUE INDEX IF NOT EXISTS idx_approval_steps_request_id_step_no
    ON approvals.approval_steps (request_id, step_no)
    WHERE (metadata->>'deleted_at') IS NULL;
