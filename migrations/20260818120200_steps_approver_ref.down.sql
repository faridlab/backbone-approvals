-- Drop the resolution-reference column (decide-time role authorization reverts to
-- assigned_to-only for role/position steps).

ALTER TABLE approvals.approval_steps
    DROP COLUMN IF EXISTS approver_ref;
