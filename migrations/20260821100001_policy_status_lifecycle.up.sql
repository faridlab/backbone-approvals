-- Migration: replace the policy lifecycle boolean with a status enum
-- approval_policies carried `is_active BOOLEAN NOT NULL DEFAULT TRUE`; the
-- tree-wide convention is one `status` enum field per lifecycle (see
-- docs/refactoring-schema in the serpa workspace). The boolean migrates only
-- rows deviating from its own column default. The hand-written
-- single-active-per-resource partial unique index rides the rename: its
-- predicate becomes `status = 'active'` with the same soft-delete carve-out.

DO $$ BEGIN
    CREATE TYPE approval_policy_status AS ENUM ('active', 'inactive');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

ALTER TABLE approvals.approval_policies ADD COLUMN status approval_policy_status NOT NULL DEFAULT 'active';
UPDATE approvals.approval_policies SET status = 'inactive' WHERE NOT is_active;
ALTER TABLE approvals.approval_policies DROP COLUMN is_active;

DROP INDEX IF EXISTS approvals.approval_policies_single_active;
CREATE UNIQUE INDEX approval_policies_single_active
    ON approvals.approval_policies (company_id, resource_type)
    WHERE status = 'active' AND (metadata->>'deleted_at') IS NULL;
