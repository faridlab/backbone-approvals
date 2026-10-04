-- Back to the soft-delete-only predicate: a rejected request holds its
-- resource again.
--
-- Going back is refused while any resource has a rejected (or cancelled)
-- request beside a newer request for the same resource, because the stricter
-- index cannot hold both. Withdraw or soft-delete the older rows first.

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM approvals.approval_requests
         WHERE (metadata->>'deleted_at') IS NULL
         GROUP BY resource_type, resource_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION 'approvals: some resources were sent again after a rejection; the stricter one-request-per-resource index cannot be restored until the older requests are soft-deleted';
    END IF;
END
$$;

DROP INDEX IF EXISTS approvals.approval_requests_one_live_per_resource;

CREATE UNIQUE INDEX IF NOT EXISTS approval_requests_one_live_per_resource
    ON approvals.approval_requests (resource_type, resource_id)
    WHERE (metadata->>'deleted_at') IS NULL;
