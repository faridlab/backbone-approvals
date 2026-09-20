-- No-op: PostgreSQL cannot drop an enum value inside a transaction. The
-- 'employee_record' value stays after a rollback; it is inert until some
-- policy names it.
SELECT 1;
