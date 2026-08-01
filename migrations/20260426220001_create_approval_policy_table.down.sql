-- Down: drop approvals.approval_policies table
DROP TABLE IF EXISTS approvals.approval_policies CASCADE;
DROP FUNCTION IF EXISTS approvals.approval_policies_audit_timestamp() CASCADE;
