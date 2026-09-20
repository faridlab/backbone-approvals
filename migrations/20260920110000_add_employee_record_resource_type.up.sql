-- Add 'employee_record' to the resource-type vocabulary: self-service
-- record-change requests file into the engine (an employee asking for a
-- change to what the company holds about them).
--
-- Append-only: PostgreSQL enum values cannot be removed inside a transaction,
-- so the DOWN migration is a documented no-op — removing the value would
-- require recreating the type, which is not worth it for a vocabulary that
-- may be reused.

ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'employee_record';
