-- Migration: append employee_record, overtime_request, resignation to approval_resource_type
--
-- The Rust enum had drifted ahead of the schema declaration (employee_record
-- and overtime_request were live in code but absent from the YAML); this
-- aligns the declared enum with the code AND adds the resignation resource
-- type — the employee-initiated notice that files into the engine and, on
-- approval, opens the offboarding. Postgres enum labels can only be
-- APPENDED; the three ride at the end, which is also their declaration order.

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_enum e JOIN pg_type t ON t.oid = e.enumtypid
                   WHERE t.typname = 'approval_resource_type' AND e.enumlabel = 'resignation') THEN
        ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'employee_record';
        ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'overtime_request';
        ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'resignation';
    END IF;
END
$$;
