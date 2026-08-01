-- Down: drop approvals.approval_step_templates table
DROP TABLE IF EXISTS approvals.approval_step_templates CASCADE;
DROP FUNCTION IF EXISTS approvals.approval_step_templates_audit_timestamp() CASCADE;
