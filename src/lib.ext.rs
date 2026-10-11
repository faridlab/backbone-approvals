//! The module's extension (hand-written; ADR-0031): what the module adds beside its
//! generated services. The generated module and builder carry `ModuleExt` and
//! `ModuleBuilderExt` and dereference to them, so their fields read as `module.field`.

#[allow(unused_imports)]
use super::*;

/// State the module adds; read through the generated module's `Deref`.
pub struct ModuleExt {
    // The decision engine (file/decide/status/withdraw). Default resolver is fail-closed:
    // dynamic approver kinds need the host to supply one at composition time.
    pub(crate) approvals_write_service: Arc<application::service::ApprovalsWriteService>,
}

/// State the builder adds.
pub struct ModuleBuilderExt {
}

impl Default for ModuleBuilderExt {
    fn default() -> Self {
        Self {
        }
    }
}

impl ModuleBuilderExt {
    /// Build the extension's state from what the generated build made.
    #[allow(unused_variables, clippy::redundant_clone)]
    pub(crate) fn build(self, parts: &ModuleParts<'_>) -> anyhow::Result<ModuleExt> {
        let db_pool = parts.db_pool.clone();
        let approvals_write_service = Arc::new(application::service::ApprovalsWriteService::new(
            db_pool.clone(),
        ));
        Ok(ModuleExt {
            approvals_write_service,
        })
    }
}

impl crate::ThisModule {
    /// The decision engine held by this module (default fail-closed resolver).
    pub fn approvals_write_service(&self) -> Arc<application::service::ApprovalsWriteService> {
        self.approvals_write_service.clone()
    }
    /// Replace the engine at the composition root — the way a host supplies a real
    /// approver resolver for the dynamic kinds. Call before mounting routes; the module is
    /// consumed once at startup.
    pub fn with_approvals_write_service(
        mut self,
        svc: Arc<application::service::ApprovalsWriteService>,
    ) -> Self {
        self.approvals_write_service = svc;
        self
    }
}
