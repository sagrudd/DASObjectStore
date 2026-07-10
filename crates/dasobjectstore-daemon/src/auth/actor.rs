#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonLocalActor {
    pub uid: u32,
    pub username: Option<String>,
    pub primary_gid: Option<u32>,
    pub groups: Vec<String>,
}

impl DaemonLocalActor {
    pub fn new(uid: u32) -> Self {
        Self {
            uid,
            username: None,
            primary_gid: None,
            groups: Vec::new(),
        }
    }

    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    pub fn with_primary_gid(mut self, primary_gid: u32) -> Self {
        self.primary_gid = Some(primary_gid);
        self
    }

    pub fn with_groups(mut self, groups: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.groups = groups.into_iter().map(Into::into).collect();
        self
    }

    pub fn has_group(&self, group: &str) -> bool {
        self.groups.iter().any(|candidate| candidate == group)
    }

    pub fn is_administrator(&self) -> bool {
        self.uid == 0 || self.has_group("dasobjectstore-admin") || self.has_group("sudo")
    }

    pub fn display_name(&self) -> String {
        self.username
            .clone()
            .unwrap_or_else(|| format!("uid:{}", self.uid))
    }
}

#[cfg(test)]
mod tests {
    use super::DaemonLocalActor;

    #[test]
    fn root_and_configured_admin_groups_are_administrators() {
        assert!(DaemonLocalActor::new(0).is_administrator());
        assert!(DaemonLocalActor::new(1000)
            .with_groups(["dasobjectstore-admin"])
            .is_administrator());
        assert!(DaemonLocalActor::new(1000)
            .with_groups(["sudo"])
            .is_administrator());
    }

    #[test]
    fn ordinary_store_writer_is_not_an_administrator() {
        assert!(!DaemonLocalActor::new(1000)
            .with_groups(["bioinformatics"])
            .is_administrator());
    }
}
