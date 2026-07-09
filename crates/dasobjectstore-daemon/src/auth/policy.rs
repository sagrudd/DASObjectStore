use super::DaemonLocalActor;
use dasobjectstore_core::ids::StoreId;
use std::fmt::{self, Display};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonStoreAccessPolicy {
    pub store_id: StoreId,
    pub reader_group: Option<String>,
    pub writer_group: Option<String>,
    pub admin_group: Option<String>,
    pub public_read: bool,
}

impl DaemonStoreAccessPolicy {
    pub fn new(store_id: StoreId) -> Self {
        Self {
            store_id,
            reader_group: None,
            writer_group: None,
            admin_group: None,
            public_read: false,
        }
    }

    pub fn with_reader_group(mut self, group: impl Into<String>) -> Self {
        self.reader_group = Some(group.into());
        self
    }

    pub fn with_writer_group(mut self, group: impl Into<String>) -> Self {
        self.writer_group = Some(group.into());
        self
    }

    pub fn with_admin_group(mut self, group: impl Into<String>) -> Self {
        self.admin_group = Some(group.into());
        self
    }

    pub fn with_public_read(mut self, public_read: bool) -> Self {
        self.public_read = public_read;
        self
    }

    pub fn validate(&self) -> Result<(), DaemonAuthorizationError> {
        reject_blank_group("reader_group", self.reader_group.as_deref())?;
        reject_blank_group("writer_group", self.writer_group.as_deref())?;
        reject_blank_group("admin_group", self.admin_group.as_deref())
    }
}

pub fn authorize_store_read(
    actor: &DaemonLocalActor,
    policy: &DaemonStoreAccessPolicy,
) -> Result<(), DaemonAuthorizationError> {
    policy.validate()?;

    if policy.public_read {
        return Ok(());
    }

    if let Some(admin_group) = &policy.admin_group {
        if actor.has_group(admin_group) {
            return Ok(());
        }
    }

    if let Some(reader_group) = &policy.reader_group {
        if actor.has_group(reader_group) {
            return Ok(());
        }
    }

    if let Some(writer_group) = &policy.writer_group {
        if actor.has_group(writer_group) {
            return Ok(());
        }
    }

    Err(DaemonAuthorizationError::ActorNotInReadGroup {
        store_id: policy.store_id.clone(),
        actor: actor.display_name(),
        reader_group: policy.reader_group.clone(),
        writer_group: policy.writer_group.clone(),
    })
}

pub fn authorize_store_write(
    actor: &DaemonLocalActor,
    policy: &DaemonStoreAccessPolicy,
) -> Result<(), DaemonAuthorizationError> {
    policy.validate()?;

    if let Some(admin_group) = &policy.admin_group {
        if actor.has_group(admin_group) {
            return Ok(());
        }
    }

    let Some(writer_group) = &policy.writer_group else {
        return Err(DaemonAuthorizationError::MissingWriterGroup {
            store_id: policy.store_id.clone(),
        });
    };

    if actor.has_group(writer_group) {
        return Ok(());
    }

    Err(DaemonAuthorizationError::ActorNotInWriterGroup {
        store_id: policy.store_id.clone(),
        actor: actor.display_name(),
        required_group: writer_group.clone(),
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonAuthorizationError {
    BlankGroup {
        field: &'static str,
    },
    MissingWriterGroup {
        store_id: StoreId,
    },
    ActorNotInReadGroup {
        store_id: StoreId,
        actor: String,
        reader_group: Option<String>,
        writer_group: Option<String>,
    },
    ActorNotInWriterGroup {
        store_id: StoreId,
        actor: String,
        required_group: String,
    },
}

impl Display for DaemonAuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankGroup { field } => write!(formatter, "{field} must not be blank"),
            Self::MissingWriterGroup { store_id } => write!(
                formatter,
                "store {store_id} does not define a writer group for daemon authorization"
            ),
            Self::ActorNotInReadGroup {
                store_id,
                actor,
                reader_group,
                writer_group,
            } => {
                write!(
                    formatter,
                    "actor {actor} is not authorized to read store {store_id}"
                )?;
                match (reader_group, writer_group) {
                    (Some(reader_group), Some(writer_group)) => write!(
                        formatter,
                        "; membership in group {reader_group} or writer group {writer_group} is required"
                    ),
                    (Some(reader_group), None) => write!(
                        formatter,
                        "; membership in group {reader_group} is required"
                    ),
                    (None, Some(writer_group)) => write!(
                        formatter,
                        "; membership in writer group {writer_group} is required"
                    ),
                    (None, None) => formatter.write_str("; no read or writer group is configured"),
                }
            }
            Self::ActorNotInWriterGroup {
                store_id,
                actor,
                required_group,
            } => write!(
                formatter,
                "actor {actor} is not authorized to write store {store_id}; membership in group {required_group} is required"
            ),
        }
    }
}

impl std::error::Error for DaemonAuthorizationError {}

fn reject_blank_group(
    field: &'static str,
    group: Option<&str>,
) -> Result<(), DaemonAuthorizationError> {
    if group.is_some_and(|value| value.trim().is_empty()) {
        return Err(DaemonAuthorizationError::BlankGroup { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        authorize_store_read, authorize_store_write, DaemonAuthorizationError,
        DaemonStoreAccessPolicy,
    };
    use crate::auth::DaemonLocalActor;
    use dasobjectstore_core::ids::StoreId;

    #[test]
    fn authorizes_public_read_for_authenticated_actor() {
        let actor = DaemonLocalActor::new(1001)
            .with_username("guest")
            .with_groups(["users"]);
        let policy = policy().with_public_read(true);

        authorize_store_read(&actor, &policy).expect("public read authorized");
    }

    #[test]
    fn authorizes_reader_group_member() {
        let actor = DaemonLocalActor::new(1001)
            .with_username("reader")
            .with_groups(["readers"]);
        let policy = policy()
            .with_reader_group("readers")
            .with_writer_group("mnemosyne");

        authorize_store_read(&actor, &policy).expect("reader group member authorized");
    }

    #[test]
    fn authorizes_writer_group_member_to_read() {
        let actor = DaemonLocalActor::new(1000)
            .with_username("stephen")
            .with_groups(["mnemosyne"]);
        let policy = policy().with_writer_group("mnemosyne");

        authorize_store_read(&actor, &policy).expect("writer group member can read");
    }

    #[test]
    fn authorizes_writer_group_member() {
        let actor = DaemonLocalActor::new(1000)
            .with_username("stephen")
            .with_groups(["mnemosyne"]);
        let policy = policy().with_writer_group("mnemosyne");

        authorize_store_write(&actor, &policy).expect("writer group member authorized");
    }

    #[test]
    fn authorizes_admin_group_member_without_writer_membership() {
        let actor = DaemonLocalActor::new(1000)
            .with_username("operator")
            .with_groups(["dasobjectstore-admin"]);
        let policy = policy()
            .with_writer_group("mnemosyne")
            .with_admin_group("dasobjectstore-admin");

        authorize_store_write(&actor, &policy).expect("admin group member authorized");
    }

    #[test]
    fn rejects_actor_outside_writer_group() {
        let actor = DaemonLocalActor::new(1001)
            .with_username("guest")
            .with_groups(["users"]);
        let policy = policy().with_writer_group("mnemosyne");

        let err = authorize_store_write(&actor, &policy).expect_err("actor rejected");

        assert_eq!(
            err,
            DaemonAuthorizationError::ActorNotInWriterGroup {
                store_id: StoreId::new("zymo").expect("store id"),
                actor: "guest".to_string(),
                required_group: "mnemosyne".to_string(),
            }
        );
    }

    #[test]
    fn rejects_actor_outside_read_and_writer_groups() {
        let actor = DaemonLocalActor::new(1001)
            .with_username("guest")
            .with_groups(["users"]);
        let policy = policy()
            .with_reader_group("readers")
            .with_writer_group("mnemosyne");

        let err = authorize_store_read(&actor, &policy).expect_err("actor rejected");

        assert_eq!(
            err,
            DaemonAuthorizationError::ActorNotInReadGroup {
                store_id: StoreId::new("zymo").expect("store id"),
                actor: "guest".to_string(),
                reader_group: Some("readers".to_string()),
                writer_group: Some("mnemosyne".to_string()),
            }
        );
    }

    #[test]
    fn rejects_missing_writer_group() {
        let actor = DaemonLocalActor::new(1000).with_groups(["mnemosyne"]);
        let policy = policy();

        let err = authorize_store_write(&actor, &policy).expect_err("policy rejected");

        assert_eq!(
            err,
            DaemonAuthorizationError::MissingWriterGroup {
                store_id: StoreId::new("zymo").expect("store id"),
            }
        );
    }

    #[test]
    fn rejects_blank_group_policy() {
        let policy = policy().with_writer_group(" ");

        let err = policy.validate().expect_err("blank group rejected");

        assert_eq!(
            err,
            DaemonAuthorizationError::BlankGroup {
                field: "writer_group"
            }
        );
    }

    fn policy() -> DaemonStoreAccessPolicy {
        DaemonStoreAccessPolicy::new(StoreId::new("zymo").expect("store id"))
    }
}
