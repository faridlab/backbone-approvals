-- Down: restore the is_active boolean and the partial unique keyed on it.
-- Only 'inactive' rows are written back as FALSE; rows at the column default
-- map to the boolean default TRUE without an UPDATE.

DROP INDEX IF EXISTS approvals.approval_policies_single_active;

ALTER TABLE approvals.approval_policies ADD COLUMN is_active BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE approvals.approval_policies SET is_active = FALSE WHERE status = 'inactive';
ALTER TABLE approvals.approval_policies DROP COLUMN status;
DROP TYPE IF EXISTS approval_policy_status;

CREATE UNIQUE INDEX approval_policies_single_active
    ON approvals.approval_policies (company_id, resource_type)
    WHERE is_active AND (metadata->>'deleted_at') IS NULL;
