-- Enum labels cannot be dropped from a Postgres enum; the down is a no-op by
-- design (an unused label is inert).
SELECT 1;
