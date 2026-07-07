use super::normalize_optional_text;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDescriptionMetadata {
    pub description: Option<String>,
    pub fields: Vec<ImportMetadataField>,
}

impl ImportDescriptionMetadata {
    pub fn new(description: Option<String>, fields: Vec<ImportMetadataField>) -> Self {
        Self {
            description: normalize_optional_text(description),
            fields,
        }
    }

    pub fn from_key_value_entries(
        description: Option<String>,
        entries: &[String],
    ) -> Result<Self, ImportMetadataError> {
        let mut fields = Vec::with_capacity(entries.len());

        for entry in entries {
            let field = ImportMetadataField::parse(entry)?;

            if fields
                .iter()
                .any(|existing: &ImportMetadataField| existing.key == field.key)
            {
                return Err(ImportMetadataError::DuplicateFieldKey { key: field.key });
            }

            fields.push(field);
        }

        Ok(Self::new(description, fields))
    }

    pub fn display_data(&self) -> ImportDescriptionMetadataDisplay {
        ImportDescriptionMetadataDisplay {
            description_label: self
                .description
                .clone()
                .unwrap_or_else(|| "not provided".to_string()),
            field_labels: self
                .fields
                .iter()
                .map(|field| format!("{}={}", field.key, field.value))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportMetadataField {
    pub key: String,
    pub value: String,
}

impl ImportMetadataField {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into().trim().to_string(),
            value: value.into().trim().to_string(),
        }
    }

    pub fn parse(entry: &str) -> Result<Self, ImportMetadataError> {
        let (key, value) =
            entry
                .split_once('=')
                .ok_or_else(|| ImportMetadataError::MissingSeparator {
                    entry: entry.to_string(),
                })?;

        let field = Self::new(key, value);
        if field.key.is_empty() {
            return Err(ImportMetadataError::BlankFieldKey {
                entry: entry.to_string(),
            });
        }
        if field.value.is_empty() {
            return Err(ImportMetadataError::BlankFieldValue {
                key: field.key.clone(),
            });
        }

        Ok(field)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDescriptionMetadataDisplay {
    pub description_label: String,
    pub field_labels: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportMetadataError {
    MissingSeparator { entry: String },
    BlankFieldKey { entry: String },
    BlankFieldValue { key: String },
    DuplicateFieldKey { key: String },
}

impl std::fmt::Display for ImportMetadataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSeparator { entry } => write!(
                formatter,
                "metadata entry `{entry}` must use KEY=VALUE syntax"
            ),
            Self::BlankFieldKey { entry } => {
                write!(formatter, "metadata entry `{entry}` has a blank key")
            }
            Self::BlankFieldValue { key } => {
                write!(formatter, "metadata field `{key}` has a blank value")
            }
            Self::DuplicateFieldKey { key } => {
                write!(
                    formatter,
                    "metadata field `{key}` was provided more than once"
                )
            }
        }
    }
}

impl std::error::Error for ImportMetadataError {}
