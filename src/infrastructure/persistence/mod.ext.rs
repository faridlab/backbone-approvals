// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The decision engine's statements (hand-authored; see metaphor.codegen.yaml user_owned).
pub mod approvals_write_repository;
pub use approvals_write_repository::ApprovalsWriteRepository;
