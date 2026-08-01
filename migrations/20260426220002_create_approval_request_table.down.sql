-- Down: drop approvals.approval_requests table
DROP TABLE IF EXISTS approvals.approval_requests CASCADE;
DROP FUNCTION IF EXISTS approvals.approval_requests_audit_timestamp() CASCADE;
