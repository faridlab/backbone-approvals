-- Record the template reference each step row was resolved FROM.
--
-- Role/position steps resolve to one concrete employee at materialization time, but the
-- engine re-checks authorization at decide time against the actor's presented role refs —
-- that check needs the ROLE (or position) id, which until now lived only in the template.
-- The column is nullable and backfills to NULL; historical rows simply keep their
-- assigned_to-only authorization.

ALTER TABLE approvals.approval_steps
    ADD COLUMN IF NOT EXISTS approver_ref UUID;
