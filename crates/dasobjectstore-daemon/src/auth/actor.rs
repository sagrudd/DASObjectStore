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

    pub fn display_name(&self) -> String {
        self.username
            .clone()
            .unwrap_or_else(|| format!("uid:{}", self.uid))
    }
}
