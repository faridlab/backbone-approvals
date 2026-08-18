-- Add 'expense' to the resource-type vocabulary (expense claims file into the engine).
--
-- Append-only: PostgreSQL enum values cannot be removed inside a transaction, so the
-- DOWN migration is a documented no-op — removing the value would require recreating
-- the type, which is not worth it for a vocabulary that may be reused.

ALTER TYPE approval_resource_type ADD VALUE IF NOT EXISTS 'expense';
