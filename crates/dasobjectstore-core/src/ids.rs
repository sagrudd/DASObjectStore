//! Strongly typed domain identifiers.

use std::fmt::{self, Display};
use std::str::FromStr;

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, InvalidId> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(InvalidId::Empty);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = InvalidId;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvalidId {
    Empty,
}

impl Display for InvalidId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("identifier must not be empty"),
        }
    }
}

impl std::error::Error for InvalidId {}

domain_id!(PoolId);
domain_id!(DiskId);
domain_id!(EnclosureId);
domain_id!(StoreId);
domain_id!(ObjectId);
domain_id!(IngestJobId);
domain_id!(PlacementId);

#[cfg(test)]
mod tests {
    use super::{DiskId, InvalidId, ObjectId};

    #[test]
    fn accepts_non_empty_identifier() {
        let id = DiskId::new("disk-a").expect("valid id");

        assert_eq!(id.as_str(), "disk-a");
        assert_eq!(id.to_string(), "disk-a");
    }

    #[test]
    fn rejects_empty_identifier() {
        let err = ObjectId::new("  ").expect_err("blank id must fail");

        assert_eq!(err, InvalidId::Empty);
    }
}
