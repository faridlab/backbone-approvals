// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The decision engine: file/decide/status/withdraw over the request+step rows the
// generated CRUD only moves. Hand-authored (see metaphor.codegen.yaml user_owned).
pub mod approvals_write_service;
pub use approvals_write_service::{
    ApprovalsError, ApprovalsWriteService, ApproverActor, ApproverResolver, Decision,
    FailClosedResolver, FileFiling, FilingOutcome,
};
// A policy's chain written and previewed whole, never row by row.
pub mod approval_chain_service;
pub use approval_chain_service::{check_chain, ChainStep, PreviewApprover, StepPreview};
