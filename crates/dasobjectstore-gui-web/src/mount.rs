use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendHost {
    Standalone,
    Monas,
    Synoptikon,
}

impl FrontendHost {
    pub fn name(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Monas => "monas",
            Self::Synoptikon => "synoptikon",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrontendMount {
    pub host: FrontendHost,
    pub base_path: String,
    pub api_base_path: String,
}

impl FrontendMount {
    pub fn default_for(host: FrontendHost) -> Self {
        let base_path = match host {
            FrontendHost::Standalone => "/products/dasobjectstore".to_string(),
            FrontendHost::Monas | FrontendHost::Synoptikon => {
                format!("/{}/dasobjectstore", host.name())
            }
        };
        Self {
            host,
            api_base_path: format!("{base_path}/api/v1"),
            base_path,
        }
    }

    pub fn auth_base_path(&self) -> String {
        format!("{}/api", self.base_path.trim_end_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::{FrontendHost, FrontendMount};

    #[test]
    fn defaults_to_host_scoped_mount_paths() {
        let mount = FrontendMount::default_for(FrontendHost::Synoptikon);

        assert_eq!(mount.base_path, "/synoptikon/dasobjectstore");
        assert_eq!(mount.api_base_path, "/synoptikon/dasobjectstore/api/v1");
    }

    #[test]
    fn standalone_mount_matches_packaged_product_route() {
        let mount = FrontendMount::default_for(FrontendHost::Standalone);

        assert_eq!(mount.base_path, "/products/dasobjectstore");
        assert_eq!(mount.api_base_path, "/products/dasobjectstore/api/v1");
        assert_eq!(mount.auth_base_path(), "/products/dasobjectstore/api");
    }

    #[test]
    fn serializes_host_names_as_snake_case() {
        let encoded = serde_json::to_value(FrontendMount::default_for(FrontendHost::Monas))
            .expect("mount serializes");

        assert_eq!(encoded["host"], "monas");
    }
}
