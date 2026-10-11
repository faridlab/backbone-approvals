// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

pub mod request_pool;
// The guarded composition (engine verbs + reads) and the operator master-data
// composer a host mounts behind its own RBAC gate.
pub use presentation::http::guarded_routes::{
    create_guarded_approvals_routes, create_guarded_approvals_routes_with,
    create_operator_master_data_routes,
};
