//! Exact-source continuity is separate from custody and ordinary object CAS.
//!
//! Contract: 79e60cf49b93e9f947c284e35cffd316f7994a75827de495f626bb7beff76a9c.
//! Parsed bytes and application mTLS authentication do not admit this purpose.

mod codec;
mod owner;

#[cfg(test)]
mod tests;

/// The installed continuity origin/backend has not been admitted in this source.
/// This narrow response does not disclose a binding or access any backend.
#[must_use]
pub fn exact_source_continuity_unavailable() -> &'static str {
    "{\"result\":\"unavailable\",\"schema\":\"dasobjectstore.exact_source_continuity_reply.v1\"}"
}

/// Non-authoritative source dispatcher. A well-formed request still receives
/// unavailable because installed purpose/backend origin is absent.
#[must_use]
pub fn exact_source_continuity_source_response(bytes: &[u8], now: u64) -> (u16, &'static str) {
    match owner::production_request(bytes, now) {
        Err(codec::Error::Denied) => (403, "{\"result\":\"denied\",\"schema\":\"dasobjectstore.exact_source_continuity_reply.v1\"}"),
        Err(codec::Error::Conflict) => (409, "{\"result\":\"conflict\",\"schema\":\"dasobjectstore.exact_source_continuity_reply.v1\"}"),
        Err(codec::Error::Unavailable) | Ok(()) => (503, exact_source_continuity_unavailable()),
    }
}

/// Source codec entry only. Caller-supplied authority never admits a purpose.
#[must_use]
pub fn exact_source_continuity_source_wire_response(
    bytes: &[u8],
    authority: &str,
    now: u64,
) -> (u16, &'static str) {
    match codec::http_body(bytes, authority) {
        Ok(body) => exact_source_continuity_source_response(body, now),
        Err(_) => (403, "{\"result\":\"denied\",\"schema\":\"dasobjectstore.exact_source_continuity_reply.v1\"}"),
    }
}
