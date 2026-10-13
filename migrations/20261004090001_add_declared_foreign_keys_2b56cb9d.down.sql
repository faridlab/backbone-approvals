-- Down: drop the declared foreign keys for approvals module
ALTER TABLE IF EXISTS approvals.approval_step_templates DROP CONSTRAINT IF EXISTS fk_approval_step_templates_policy_id;
ALTER TABLE IF EXISTS approvals.approval_steps DROP CONSTRAINT IF EXISTS fk_approval_steps_request_id;
ALTER TABLE IF EXISTS approvals.approval_requests DROP CONSTRAINT IF EXISTS fk_approval_requests_policy_id;
