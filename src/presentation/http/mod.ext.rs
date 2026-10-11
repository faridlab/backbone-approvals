// This directory's extension (hand-written; ADR-0031). The generated mod.rs (lib.rs for
// src/) pulls it in with include!, so its lines resolve against this directory.

// The guarded composition (operator CRUD + engine reads + decide/withdraw verbs).
// Hand-authored (see metaphor.codegen.yaml user_owned).
pub mod guarded_routes;
pub use guarded_routes::{
    ActorResolver, SubAsEmployee, create_guarded_approvals_routes, create_guarded_approvals_routes_with, set_actor_resolver,
    create_operator_master_data_routes,
};
