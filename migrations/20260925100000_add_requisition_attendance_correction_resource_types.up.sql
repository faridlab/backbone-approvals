-- Migration: append requisition and attendance_correction to approval_resource_type
--
-- Two new workflow kinds file into the engine: a requisition (headcount and
-- budget before a vacancy opens) and an attendance correction (an employee's
-- own clock-session edit, applied only when approved). Postgres enum labels
-- can only be APPENDED; both ride at the end, matching declaration order.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid
                   WHERE t.typname = 'approval_resource_type' AND e.enumlabel = 'attendance_correction') THEN
        ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'requisition';
        ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'attendance_correction';
    END IF;
END
$$;
