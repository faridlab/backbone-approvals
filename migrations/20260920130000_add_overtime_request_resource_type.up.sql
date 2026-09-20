-- Add 'overtime_request' to the resource-type vocabulary: overtime
-- pre-authorisation (written authorisation BEFORE the hours are worked)
-- files into the engine.
--
-- Append-only: PostgreSQL enum values cannot be removed inside a
-- transaction, so the DOWN migration is a documented no-op.

ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'overtime_request';
