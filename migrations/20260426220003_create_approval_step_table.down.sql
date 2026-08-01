-- Down: drop approvals.approval_steps table
DROP TABLE IF EXISTS approvals.approval_steps CASCADE;
DROP FUNCTION IF EXISTS approvals.approval_steps_audit_timestamp() CASCADE;
