use serde::{Deserialize, Serialize};

pub const HOST_STORAGE_BOUNDARY_SCHEMA_VERSION: &str = "mnemosyne.host_storage_boundary.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HostMode {
    SynoptikonIntegrated,
    MonasStandalone,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateAuthority {
    SynoptikonRdbms,
    MonasLocalFiles,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtefactAuthority {
    SynoptikonObjectStore,
    MonasLocalProductTree,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum LocalRootTemplate {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "/opt/<productName>")]
    OptProductName,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalRootPolicy {
    pub allowed: bool,
    pub root_template: LocalRootTemplate,
    pub durable_in_integrated_mode: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlRequiredBackend {
    SynoptikonRdbms,
    None,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SqlPolicy {
    pub required_backend: SqlRequiredBackend,
    pub sqlite_allowed: bool,
    pub network_sql_allowed: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RegistrationContract {
    #[serde(rename = "mnemosyne.internal.artefact_registration.v1")]
    MnemosyneInternalArtefactRegistrationV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectStorePolicy {
    pub required: bool,
    pub direct_product_write_allowed: bool,
    pub registration_contract: Option<RegistrationContract>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostStorageBoundary {
    pub schema_version: String,
    pub host_mode: HostMode,
    pub state_authority: StateAuthority,
    pub artefact_authority: ArtefactAuthority,
    pub local_root_policy: LocalRootPolicy,
    pub sql_policy: SqlPolicy,
    pub object_store_policy: ObjectStorePolicy,
}

pub fn synoptikon_object_store_boundary() -> HostStorageBoundary {
    HostStorageBoundary {
        schema_version: HOST_STORAGE_BOUNDARY_SCHEMA_VERSION.to_string(),
        host_mode: HostMode::SynoptikonIntegrated,
        state_authority: StateAuthority::SynoptikonRdbms,
        artefact_authority: ArtefactAuthority::SynoptikonObjectStore,
        local_root_policy: LocalRootPolicy {
            allowed: false,
            root_template: LocalRootTemplate::None,
            durable_in_integrated_mode: false,
        },
        sql_policy: SqlPolicy {
            required_backend: SqlRequiredBackend::SynoptikonRdbms,
            sqlite_allowed: false,
            network_sql_allowed: true,
        },
        object_store_policy: ObjectStorePolicy {
            required: true,
            direct_product_write_allowed: false,
            registration_contract: Some(
                RegistrationContract::MnemosyneInternalArtefactRegistrationV1,
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        synoptikon_object_store_boundary, ArtefactAuthority, HostMode, LocalRootTemplate,
        RegistrationContract, SqlRequiredBackend, StateAuthority,
        HOST_STORAGE_BOUNDARY_SCHEMA_VERSION,
    };
    use serde_json::json;

    #[test]
    fn builds_synoptikon_object_store_boundary() {
        let boundary = synoptikon_object_store_boundary();

        assert_eq!(
            boundary.schema_version,
            HOST_STORAGE_BOUNDARY_SCHEMA_VERSION
        );
        assert_eq!(boundary.host_mode, HostMode::SynoptikonIntegrated);
        assert_eq!(boundary.state_authority, StateAuthority::SynoptikonRdbms);
        assert_eq!(
            boundary.artefact_authority,
            ArtefactAuthority::SynoptikonObjectStore
        );
        assert_eq!(
            boundary.local_root_policy.root_template,
            LocalRootTemplate::None
        );
        assert_eq!(
            boundary.sql_policy.required_backend,
            SqlRequiredBackend::SynoptikonRdbms
        );
        assert_eq!(
            boundary.object_store_policy.registration_contract,
            Some(RegistrationContract::MnemosyneInternalArtefactRegistrationV1)
        );
    }

    #[test]
    fn serializes_with_mnemosyne_contract_names() {
        let serialized =
            serde_json::to_value(synoptikon_object_store_boundary()).expect("boundary serializes");

        assert_eq!(
            serialized,
            json!({
                "schema_version": "mnemosyne.host_storage_boundary.v1",
                "host_mode": "synoptikon_integrated",
                "state_authority": "synoptikon_rdbms",
                "artefact_authority": "synoptikon_object_store",
                "local_root_policy": {
                    "allowed": false,
                    "root_template": "none",
                    "durable_in_integrated_mode": false
                },
                "sql_policy": {
                    "required_backend": "synoptikon_rdbms",
                    "sqlite_allowed": false,
                    "network_sql_allowed": true
                },
                "object_store_policy": {
                    "required": true,
                    "direct_product_write_allowed": false,
                    "registration_contract": "mnemosyne.internal.artefact_registration.v1"
                }
            })
        );
    }
}
