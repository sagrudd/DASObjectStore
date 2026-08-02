# DASObjectStore reference decoder

`dasobjectstore-reference` is a small, source-distributable Rust library for
strictly decoding and validating the non-secret `ObjectRefV1` and
`EvidenceRefV1` values defined by DASObjectStore ADR-0004.

It validates the bounded v1 JSON grammar and canonical domain digests. It does
not issue or resolve references, open a transport, access storage, hold a
credential or capability, or make an authority decision.

The crate is intentionally prepared for downstream source packaging only. It
is not published to crates.io, and accepting a reference is not proof that its
object exists, that an actor is authorised, or that a storage operation may be
performed.

## Downstream source use

Use an exact source revision in the consuming product's dependency policy, for
example:

```toml
[dependencies]
dasobjectstore-reference = { git = "https://github.com/sagrudd/DASObjectStore.git", rev = "<reviewed-commit>" }
```

The packaged `fixtures/downstream-consumer` template is an offline consumer
fixture. Renaming `Cargo.toml.template` to `Cargo.toml` after extracting the
source package proves that the crate remains independently usable; it is not
an issuance, storage, transport, or release qualification.
