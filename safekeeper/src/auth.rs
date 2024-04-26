use utils::auth::{AuthError, Claims, Scope};
use utils::id::TenantId;

pub fn check_permission(claims: &Claims, tenant_id: Option<TenantId>) -> Result<(), AuthError> {
    match (&claims.scope, tenant_id) {
        (Scope::Tenant, None) => Err(AuthError(
            "Attempt to access management api with tenant scope. Permission denied".into(),
        )),
        (Scope::Tenant, Some(tenant_id)) => {
            if claims.tenant_id.unwrap() != tenant_id {
                return Err(AuthError("Tenant id mismatch. Permission denied".into()));
            }
            Ok(())
        }
        (Scope::PageServerApi | Scope::GenerationsApi, _) => Err(AuthError(
            format!(
                "JWT scope '{:?}' is ineligible for Safekeeper auth",
                claims.scope
            )
            .into(),
        )),
        (Scope::SafekeeperData, _) => Ok(()),
    }
}
