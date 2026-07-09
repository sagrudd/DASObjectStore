use prosopikon_core::{
    HostAdapterProfile, InMemoryProsopikonStore, ProsopikonSnapshot, TenantUserRelationship,
};

/// Returns the DASObjectStore Prosopikon adapter profile.
///
/// Prosopikon is the identity and entitlement authority. DASObjectStore remains
/// the final authority for storage mutations through `dasobjectstored`.
#[must_use]
pub fn dasobjectstore_prosopikon_profile() -> HostAdapterProfile {
    HostAdapterProfile::dasobjectstore()
}

#[must_use]
pub fn dasobjectstore_prosopikon_snapshot() -> ProsopikonSnapshot {
    prosopikon_store().snapshot()
}

#[must_use]
pub fn dasobjectstore_prosopikon_relationships() -> Vec<TenantUserRelationship> {
    prosopikon_store().relationships()
}

fn prosopikon_store() -> InMemoryProsopikonStore {
    InMemoryProsopikonStore::new(dasobjectstore_prosopikon_profile())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prosopikon_core::{
        ProsopikonAuthenticationFramework, ProsopikonDeviceTokenRequirement, ProsopikonHost,
        ProsopikonStorageBackend,
    };

    #[test]
    fn dasobjectstore_prosopikon_profile_keeps_storage_mutation_with_daemon() {
        let profile = dasobjectstore_prosopikon_profile();

        assert_eq!(profile.host, ProsopikonHost::DasObjectStore);
        assert_eq!(profile.identity_authority, "prosopikon");
        assert_eq!(
            profile.authentication_framework,
            ProsopikonAuthenticationFramework::Hybrid
        );
        assert_eq!(
            profile.device_token_requirement,
            ProsopikonDeviceTokenRequirement::NotRequired
        );
        assert_eq!(
            profile.storage_backend,
            ProsopikonStorageBackend::DasObjectStoreDaemon
        );
    }
}
