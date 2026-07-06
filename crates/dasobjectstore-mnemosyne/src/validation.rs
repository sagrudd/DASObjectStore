pub(crate) fn is_uuid_like(value: &str) -> bool {
    let parts = value.trim().split('-').collect::<Vec<_>>();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(parts.iter())
            .all(|(expected_len, part)| {
                part.len() == *expected_len && part.chars().all(|ch| ch.is_ascii_hexdigit())
            })
}

pub(crate) fn validate_context_id_like(field: &'static str, value: &str) -> Result<(), FieldError> {
    if value.is_empty() || value.len() > 128 {
        return Err(FieldError::new(field, "must be 1-128 characters"));
    }

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(FieldError::new(field, "must not be empty"));
    };
    if !first.is_ascii_alphanumeric() {
        return Err(FieldError::new(
            field,
            "must start with an ASCII letter or digit",
        ));
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-')) {
        return Err(FieldError::new(field, "contains unsupported characters"));
    }
    Ok(())
}

pub(crate) fn validate_role_like(field: &'static str, role: &str) -> Result<(), FieldError> {
    if role.is_empty() || role.len() > 96 {
        return Err(FieldError::new(field, "must be 1-96 characters"));
    }

    let mut chars = role.chars();
    let Some(first) = chars.next() else {
        return Err(FieldError::new(field, "must not be empty"));
    };
    if !first.is_ascii_lowercase() {
        return Err(FieldError::new(
            field,
            "must start with a lowercase ASCII letter",
        ));
    }
    if !chars
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | ':' | '-'))
    {
        return Err(FieldError::new(field, "contains unsupported characters"));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FieldError {
    pub field: &'static str,
    pub reason: String,
}

impl FieldError {
    fn new(field: &'static str, reason: &str) -> Self {
        Self {
            field,
            reason: reason.to_string(),
        }
    }
}
