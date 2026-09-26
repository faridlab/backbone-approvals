-- Migration: append discipline to approval_resource_type
--
-- The employee's contest of a warning letter (SP1-SP3) files into the engine
-- as this kind: an upheld contest cancels the record, a refused one returns
-- it to acknowledged-pending. Postgres enum labels can only be APPENDED; the
-- label rides at the end, matching declaration order.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid
                   WHERE t.typname = 'approval_resource_type' AND e.enumlabel = 'discipline') THEN
        ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'discipline';
    END IF;
END
$$;
