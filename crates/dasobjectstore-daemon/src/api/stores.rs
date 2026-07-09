use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::StorePolicy;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreInventoryRequest {
    pub include_policy: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreInventoryResponse {
    pub stores: Vec<StoreInventoryItem>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreInventoryItem {
    pub store_id: StoreId,
    pub policy: StorePolicy,
    pub bucket_name: Option<String>,
    pub reader_group: Option<String>,
    pub writer_group: Option<String>,
    pub public: bool,
    pub writable: bool,
}

#[cfg(test)]
mod tests {
    use super::{StoreInventoryItem, StoreInventoryResponse};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};

    #[test]
    fn store_inventory_serializes_store_policy_once() {
        let response = StoreInventoryResponse {
            stores: vec![StoreInventoryItem {
                store_id: StoreId::new("zymo").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
                bucket_name: Some("dos-zymo".to_string()),
                reader_group: Some("mnemosyne-readers".to_string()),
                writer_group: Some("mnemosyne".to_string()),
                public: false,
                writable: true,
            }],
        };

        let encoded = serde_json::to_value(response).expect("inventory serializes");

        assert_eq!(encoded["stores"][0]["store_id"], "zymo");
        assert_eq!(encoded["stores"][0]["reader_group"], "mnemosyne-readers");
        assert_eq!(encoded["stores"][0]["writer_group"], "mnemosyne");
        assert_eq!(encoded["stores"][0]["public"], false);
        assert_eq!(encoded["stores"][0]["policy"]["class"], "ReproducibleCache");
    }
}
