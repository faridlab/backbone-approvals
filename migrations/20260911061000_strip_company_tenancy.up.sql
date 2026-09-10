-- Hand-authored (user-owned). Not regenerated.
--
-- Strip every company-fence artifact from the approvals tables (ADR-0029): the module is
-- tenant-agnostic; org scoping is installed by the COMPOSING service's tenancy decorator,
-- never by the module. Dropped here, per table: the company-leading indexes, the
-- <table>_company_isolation RLS policy, and the company_id column itself.
--
-- Tables: approval_policies, approval_requests, approval_steps, approval_step_templates,
-- delegations.
--
-- Ordering guard (the decorator must run FIRST on any database with data): the module
-- never moves tenancy data. A table is safe to strip when EITHER
--   a) it carries org_unit_id with no NULLs — the decorator backfilled it from company_id —
--      or b) it is empty (a fresh database: the earlier chain files created it empty).
-- Otherwise the strip RAISEs, naming the decorator step, rather than dropping a column
-- that still holds the only tenancy key. The file is re-runnable (every drop is IF EXISTS
-- and the tracker has no checksums), so a failed run retries cleanly after the decorator
-- lands.
--
-- RLS enable/force flags are deliberately NOT touched: the decorator owns those now.
-- The tenant-free domain artifacts stay: every remaining index (approval_requests on
-- requested_by / resource_type + resource_id, approval_steps on request_id /
-- assigned_to + status, approval_step_templates on policy_id, delegations on
-- approver_id / delegate_to_id, plus the soft-delete-scoped partial uniques on
-- (policy_id, step_no) and (request_id, step_no)) keys no tenant column. The
-- per-resource identity unique — one LIVE request per resource — is RESTORED here
-- tenant-free: a resource id names exactly one unit's record, so a second unit's live
-- chain for the same resource is cross-tenant interference, and an org-scoped form
-- would admit exactly that. The posture uniques — one policy name per resource family,
-- one active policy per resource — stay with the composing decorator's org-scoped
-- declarations: the pre-fence company-scoped forms would be meaningless once the
-- column is gone, and global forms would forbid two units of one tenant from keeping
-- their own approval chains.

DO $$
DECLARE
    t text;
    has_org boolean;
    org_nulls bigint;
    total bigint;
    offenders text := '';
BEGIN
    FOREACH t IN ARRAY ARRAY['approval_policies', 'approval_requests', 'approval_steps', 'approval_step_templates', 'delegations']
    LOOP
        IF to_regclass(format('approvals.%I', t)) IS NULL THEN
            CONTINUE; -- chain not fully applied on this database; nothing to strip
        END IF;

        SELECT EXISTS (
                   SELECT 1 FROM information_schema.columns
                   WHERE table_schema = 'approvals' AND table_name = t AND column_name = 'org_unit_id'
               )
        INTO has_org;

        EXECUTE format('SELECT count(*) FROM approvals.%I', t) INTO total;

        IF has_org THEN
            EXECUTE format(
                'SELECT count(*) FROM approvals.%I WHERE org_unit_id IS NULL', t)
            INTO org_nulls;
        ELSE
            org_nulls := total; -- no org column: every row's only tenancy key is company_id
        END IF;

        IF has_org AND org_nulls = 0 THEN
            CONTINUE; -- decorator backfilled: safe
        END IF;
        IF total = 0 THEN
            CONTINUE; -- empty table (fresh database): safe
        END IF;
        offenders := offenders || format(' approvals.%s (%s rows, %s rows not covered by org_unit_id);', t, total, org_nulls);
    END LOOP;

    IF offenders <> '' THEN
        RAISE EXCEPTION 'refusing to strip company_id — these tables are not yet covered by the tenancy decorator:%. Apply the composing service''s tenancy decorator (it backfills org_unit_id from company_id) and re-run; it is the only step that moves tenancy data.', offenders;
    END IF;
END $$;

-- ── approval_policies ─────────────────────────────────────────────────────────
DROP INDEX IF EXISTS approvals.idx_approval_policies_company_id;
DROP INDEX IF EXISTS approvals.idx_approval_policies_company_id_resource_type;
DROP INDEX IF EXISTS approvals.idx_approval_policies_company_id_resource_type_name;
DROP INDEX IF EXISTS approvals.approval_policies_single_active;
DROP POLICY IF EXISTS approval_policies_company_isolation ON approvals.approval_policies;
ALTER TABLE approvals.approval_policies DROP COLUMN IF EXISTS company_id;

-- ── approval_requests ─────────────────────────────────────────────────────────
DROP INDEX IF EXISTS approvals.idx_approval_requests_company_id_status;
DROP INDEX IF EXISTS approvals.idx_approval_requests_company_id_resource_type_resource_id;
DROP POLICY IF EXISTS approval_requests_company_isolation ON approvals.approval_requests;
ALTER TABLE approvals.approval_requests DROP COLUMN IF EXISTS company_id;

-- One LIVE request per resource, tenant-free (resource identity — see the header):
-- the company-scoped fence is replaced by a strictly stronger guarantee, since a
-- resource id cannot belong to two units at once. Withdrawal soft-deletes the row,
-- which frees the slot for a fresh chain, exactly as before the strip.
CREATE UNIQUE INDEX IF NOT EXISTS approval_requests_one_live_per_resource
    ON approvals.approval_requests (resource_type, resource_id)
    WHERE (metadata->>'deleted_at') IS NULL;

-- ── approval_steps ────────────────────────────────────────────────────────────
DROP POLICY IF EXISTS approval_steps_company_isolation ON approvals.approval_steps;
ALTER TABLE approvals.approval_steps DROP COLUMN IF EXISTS company_id;

-- ── approval_step_templates ───────────────────────────────────────────────────
DROP POLICY IF EXISTS approval_step_templates_company_isolation ON approvals.approval_step_templates;
ALTER TABLE approvals.approval_step_templates DROP COLUMN IF EXISTS company_id;

-- ── delegations ───────────────────────────────────────────────────────────────
DROP INDEX IF EXISTS approvals.idx_delegations_company_id_status;
DROP POLICY IF EXISTS delegations_company_isolation ON approvals.delegations;
ALTER TABLE approvals.delegations DROP COLUMN IF EXISTS company_id;
