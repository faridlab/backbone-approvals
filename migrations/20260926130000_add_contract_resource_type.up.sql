-- Migration: append contract to approval_resource_type
--
-- The employment-contract decision (renew / convert / end) files into the
-- engine as this kind; the outcome applies only on the approved verdict (the
-- settlement dispatcher drives it). Labels can only be APPENDED.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid
                   WHERE t.typname = 'approval_resource_type' AND e.enumlabel = 'contract') THEN
        ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'contract';
    END IF;
END
$$;
