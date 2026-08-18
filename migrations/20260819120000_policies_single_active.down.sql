-- Restore the pre-index posture (several active policies per resource possible again;
-- the engine picks deterministically — earliest created, then lowest id).

DROP INDEX IF EXISTS approvals.approval_policies_single_active;
