-- A decided-negative request no longer holds its resource.
--
-- The per-resource partial unique index keeps at most one request per
-- (resource_type, resource_id) that still holds the resource, and `file`
-- returns that request instead of filing a new one. Until now only the
-- soft-delete stamped by `withdraw` released the resource, so a REJECTED
-- request held it forever: a consumer that sent the same record again after
-- a rejection (a timesheet month, for one) got the old rejected request back,
-- and no approver ever saw the new submission.
--
-- The index now leaves out every terminal-negative request:
--   rejected  — an approver refused it; the requester may send again.
--   withdrawn — the requester pulled it back (also soft-deleted by withdraw).
--   cancelled — ended without a decision; the record may be sent again.
-- Pending and approved requests keep holding the resource, so a re-file of a
-- request still in flight, or of one already granted, converges on it as
-- before. Draft is never written by the engine and keeps holding it too.
--
-- Loosening only: every row the old index admitted is still admitted, so the
-- rebuild cannot hit a duplicate. Hand-written: the schema DSL cannot express
-- a partial unique index; schema/models is unaffected (no column changes).

DROP INDEX IF EXISTS approvals.approval_requests_one_live_per_resource;

CREATE UNIQUE INDEX IF NOT EXISTS approval_requests_one_live_per_resource
    ON approvals.approval_requests (resource_type, resource_id)
    WHERE (metadata->>'deleted_at') IS NULL
      AND status NOT IN ('rejected', 'withdrawn', 'cancelled');
