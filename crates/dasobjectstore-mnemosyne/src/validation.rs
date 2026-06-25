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
