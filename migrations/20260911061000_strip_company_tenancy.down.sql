-- Hand-authored (user-owned). Not regenerated.
--
-- Best-effort restore sketch for the tenancy strip (ADR-0029). This is a breaking module
-- release against dev-stage databases: the down re-adds the company_id column as nullable
-- with its plain indexes and the company isolation policy shape, but restores NO data —
-- rows written after the strip (or after the decorator re-keyed them) carry org_unit_id
-- only. The tenant-free per-resource identity unique is dropped (the company-scoped form
-- returns below); the posture uniques (the per-resource-family policy-name unique and the
-- one-active-policy rule) are NOT restored: the composing service's tenancy decorator owns
-- their org-scoped re-declarations now. The decorator remains the live fence; treat this
-- down as a schema-shape sketch for archaeology, not a usable rollback.

DROP INDEX IF EXISTS approvals.approval_requests_one_live_per_resource;

ALTER TABLE approvals.approval_policies        ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE approvals.approval_requests        ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE approvals.approval_steps           ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE approvals.approval_step_templates  ADD COLUMN IF NOT EXISTS company_id uuid;
ALTER TABLE approvals.delegations              ADD COLUMN IF NOT EXISTS company_id uuid;

CREATE INDEX IF NOT EXISTS idx_approval_policies_company_id
    ON approvals.approval_policies (company_id);
CREATE INDEX IF NOT EXISTS idx_approval_policies_company_id_resource_type
    ON approvals.approval_policies (company_id, resource_type);
CREATE INDEX IF NOT EXISTS idx_approval_requests_company_id_status
    ON approvals.approval_requests (company_id, status);
CREATE INDEX IF NOT EXISTS idx_delegations_company_id_status
    ON approvals.delegations (company_id, status);

CREATE POLICY approval_policies_company_isolation ON approvals.approval_policies
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY approval_requests_company_isolation ON approvals.approval_requests
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY approval_steps_company_isolation ON approvals.approval_steps
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY approval_step_templates_company_isolation ON approvals.approval_step_templates
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
CREATE POLICY delegations_company_isolation ON approvals.delegations
    FOR ALL
    USING      (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid)
    WITH CHECK (company_id = NULLIF(current_setting('app.company_id', true), '')::uuid);
