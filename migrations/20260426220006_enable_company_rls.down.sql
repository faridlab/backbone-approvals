-- Down: remove the company RLS fence for approvals module

-- Reverse the company RLS fence for approvals.approval_policies
DROP POLICY IF EXISTS approval_policies_company_isolation ON approvals.approval_policies;
ALTER TABLE approvals.approval_policies NO FORCE ROW LEVEL SECURITY;
ALTER TABLE approvals.approval_policies DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for approvals.approval_requests
DROP POLICY IF EXISTS approval_requests_company_isolation ON approvals.approval_requests;
ALTER TABLE approvals.approval_requests NO FORCE ROW LEVEL SECURITY;
ALTER TABLE approvals.approval_requests DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for approvals.approval_steps
DROP POLICY IF EXISTS approval_steps_company_isolation ON approvals.approval_steps;
ALTER TABLE approvals.approval_steps NO FORCE ROW LEVEL SECURITY;
ALTER TABLE approvals.approval_steps DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for approvals.approval_step_templates
DROP POLICY IF EXISTS approval_step_templates_company_isolation ON approvals.approval_step_templates;
ALTER TABLE approvals.approval_step_templates NO FORCE ROW LEVEL SECURITY;
ALTER TABLE approvals.approval_step_templates DISABLE ROW LEVEL SECURITY;

-- Reverse the company RLS fence for approvals.delegations
DROP POLICY IF EXISTS delegations_company_isolation ON approvals.delegations;
ALTER TABLE approvals.delegations NO FORCE ROW LEVEL SECURITY;
ALTER TABLE approvals.delegations DISABLE ROW LEVEL SECURITY;

