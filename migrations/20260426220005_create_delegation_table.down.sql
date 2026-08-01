-- Down: drop approvals.delegations table
DROP TABLE IF EXISTS approvals.delegations CASCADE;
DROP FUNCTION IF EXISTS approvals.delegations_audit_timestamp() CASCADE;
