-- Down: drop enum types for approvals module
DROP TYPE IF EXISTS delegation_status CASCADE;
DROP TYPE IF EXISTS approver_kind CASCADE;
DROP TYPE IF EXISTS approval_step_status CASCADE;
DROP TYPE IF EXISTS approval_status CASCADE;
DROP TYPE IF EXISTS approval_priority CASCADE;
DROP TYPE IF EXISTS approval_resource_type CASCADE;
