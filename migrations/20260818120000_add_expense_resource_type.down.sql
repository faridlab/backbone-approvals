-- No-op: PostgreSQL cannot drop an enum value inside a transaction. The 'expense' value
-- stays in the vocabulary; it is harmless when unused.
SELECT 1;
