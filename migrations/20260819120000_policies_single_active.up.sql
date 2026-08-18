-- One ACTIVE policy per company and resource type.
--
-- With several active policies for the same resource, the engine's deterministic
-- earliest-created pick made a replacement policy a silent no-op: an operator could
-- create and activate a new policy while the old one stayed active and no filing would
-- ever route through it. The partial unique index refuses the second active row at
-- write time. Deactivate (or soft-delete) the previous policy before activating a
-- replacement.
--
-- Hand-written: the schema DSL cannot express a partial unique index over a boolean +
-- JSONB condition; schema/models/approval_policy.model.yaml is unaffected (no column
-- changes).

CREATE UNIQUE INDEX IF NOT EXISTS approval_policies_single_active
    ON approvals.approval_policies (company_id, resource_type)
    WHERE is_active AND (metadata->>'deleted_at') IS NULL;
