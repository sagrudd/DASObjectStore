#!/usr/bin/env python3
"""Verify the review-only ObjectRef/EvidenceRef v1 contract vectors.

This is deliberately a fixture verifier, not a DASObjectStore implementation.
It has no network, storage, capability, or authority side effects.  The checks
mirror only the lexical, structural, canonicalisation, and domain-digest rules
frozen in ADR-0004 so independent implementations can compare their vectors
before the owner-side API is accepted.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
from copy import deepcopy
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_DIR = ROOT / "docs" / "adr" / "fixtures"
MAX_ENCODED_BYTES = 8192
MAX_SAFE_INTEGER = 9_007_199_254_740_991
IDENTIFIER = re.compile(r"^[a-z0-9][a-z0-9._-]{0,127}$")
DIGEST = re.compile(r"^[0-9a-f]{64}$")


class DuplicateMember(ValueError):
    """Raised when JSON contains duplicate decoded member names."""


def reject_duplicate_members(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise DuplicateMember(key)
        result[key] = value
    return result


def parse_json(raw: bytes) -> Any:
    if len(raw) > MAX_ENCODED_BYTES:
        raise ValueError("encoded reference exceeds 8192-byte bound")
    return json.loads(raw.decode("utf-8"), object_pairs_hook=reject_duplicate_members)


def canonical_bytes(value: Any) -> bytes:
    # All fixture strings are ASCII and all numbers are bounded integers, so
    # this is the RFC 8785/JCS form for the ADR-0004 fixture subset.
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode(
        "utf-8"
    )


def require_keys(value: Any, expected: set[str], label: str) -> None:
    if not isinstance(value, dict) or set(value) != expected:
        raise ValueError(f"{label} members differ from the ADR-0004 v1 set")


def require_identifier(value: Any, label: str) -> None:
    if not isinstance(value, str) or not value.isascii() or IDENTIFIER.fullmatch(value) is None:
        raise ValueError(f"{label} is not a bounded canonical identifier")
    if any(character in value for character in ("/", "\\", ":", "%")):
        raise ValueError(f"{label} contains path or URL syntax")


def require_digest(value: Any, label: str) -> None:
    require_keys(value, {"algorithm", "value"}, label)
    if value["algorithm"] != "sha256" or not isinstance(value["value"], str):
        raise ValueError(f"{label} is not a sha256 digest")
    if DIGEST.fullmatch(value["value"]) is None:
        raise ValueError(f"{label} is not lowercase hexadecimal")


def require_integer(value: Any, label: str, *, minimum: int = 0) -> None:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"{label} is not an integer")
    if value < minimum or value > MAX_SAFE_INTEGER:
        raise ValueError(f"{label} is outside the safe-integer bound")


def validate_scope(value: Any) -> None:
    require_keys(value, {"installation_id", "site_trust_domain_id", "tenant_id", "project_id"}, "authority_scope")
    for key, member in value.items():
        require_identifier(member, f"authority_scope.{key}")


def validate_object_ref(value: Any) -> None:
    require_keys(
        value,
        {
            "schema",
            "authority_scope",
            "store_id",
            "object_id",
            "object_version",
            "size_bytes",
            "content_digest",
            "domain_digest",
        },
        "ObjectRefV1",
    )
    if value["schema"] != "dasobjectstore.object_ref.v1":
        raise ValueError("unexpected ObjectRef schema")
    validate_scope(value["authority_scope"])
    require_identifier(value["store_id"], "store_id")
    require_identifier(value["object_id"], "object_id")
    require_integer(value["object_version"], "object_version", minimum=1)
    require_integer(value["size_bytes"], "size_bytes")
    require_digest(value["content_digest"], "content_digest")
    require_digest(value["domain_digest"], "domain_digest")

    identity = deepcopy(value)
    expected_digest = identity.pop("domain_digest")["value"]
    digest = hashlib.sha256(
        b"DASOBJECTSTORE_OBJECT_REF_V1\0" + canonical_bytes(identity)
    ).hexdigest()
    if digest != expected_digest:
        raise ValueError(f"ObjectRef domain digest mismatch: expected {digest}")


def validate_evidence_ref(value: Any) -> None:
    require_keys(
        value,
        {
            "schema",
            "object_ref",
            "evidence_kind",
            "evidence_revision",
            "subject_digest",
            "domain_digest",
        },
        "EvidenceRefV1",
    )
    if value["schema"] != "dasobjectstore.evidence_ref.v1":
        raise ValueError("unexpected EvidenceRef schema")
    validate_object_ref(value["object_ref"])
    require_identifier(value["evidence_kind"], "evidence_kind")
    require_integer(value["evidence_revision"], "evidence_revision", minimum=1)
    require_digest(value["subject_digest"], "subject_digest")
    require_digest(value["domain_digest"], "domain_digest")

    identity = deepcopy(value)
    expected_digest = identity.pop("domain_digest")["value"]
    digest = hashlib.sha256(
        b"DASOBJECTSTORE_EVIDENCE_REF_V1\0" + canonical_bytes(identity)
    ).hexdigest()
    if digest != expected_digest:
        raise ValueError(f"EvidenceRef domain digest mismatch: expected {digest}")


def load_fixture(name: str) -> tuple[bytes, Any]:
    path = FIXTURE_DIR / name
    raw = path.read_bytes()
    return raw, parse_json(raw)


def expect_rejected(action: Any, label: str) -> None:
    try:
        action()
    except (DuplicateMember, UnicodeDecodeError, ValueError, json.JSONDecodeError):
        return
    raise AssertionError(f"negative fixture check was accepted: {label}")


def verify() -> None:
    object_raw, object_ref = load_fixture("object-ref-v1.json")
    evidence_raw, evidence_ref = load_fixture("evidence-ref-v1.json")
    validate_object_ref(object_ref)
    validate_evidence_ref(evidence_ref)
    if object_raw != canonical_bytes(object_ref) + b"\n":
        raise AssertionError("ObjectRef fixture is not emitted in canonical JCS form")
    if evidence_raw != canonical_bytes(evidence_ref) + b"\n":
        raise AssertionError("EvidenceRef fixture is not emitted in canonical JCS form")

    reordered = json.dumps(
        {"schema": object_ref["schema"], "authority_scope": object_ref["authority_scope"], **{
            key: object_ref[key]
            for key in ("store_id", "object_id", "object_version", "size_bytes", "content_digest", "domain_digest")
        }},
        separators=(",", ":"),
    ).encode()
    reordered_ref = parse_json(reordered)
    validate_object_ref(reordered_ref)
    if canonical_bytes(reordered_ref) != canonical_bytes(object_ref):
        raise AssertionError("member reordering changed canonical identity")

    duplicate = b'{"schema":"dasobjectstore.object_ref.v1","schema":"dasobjectstore.object_ref.v1"}'
    expect_rejected(lambda: parse_json(duplicate), "duplicate decoded member")

    unknown = deepcopy(object_ref)
    unknown["unexpected"] = "extension"
    expect_rejected(lambda: validate_object_ref(unknown), "unknown member")

    uppercase_digest = deepcopy(object_ref)
    uppercase_digest["content_digest"]["value"] = "A" * 64
    expect_rejected(lambda: validate_object_ref(uppercase_digest), "uppercase digest")

    path_value = deepcopy(object_ref)
    path_value["store_id"] = "../../outside"
    expect_rejected(lambda: validate_object_ref(path_value), "path-shaped identifier")

    scope_drift = deepcopy(evidence_ref)
    scope_drift["object_ref"]["authority_scope"]["project_id"] = "other-project"
    expect_rejected(lambda: validate_evidence_ref(scope_drift), "scope-drifted digest")


if __name__ == "__main__":
    try:
        verify()
    except (AssertionError, DuplicateMember, UnicodeDecodeError, ValueError, json.JSONDecodeError) as error:
        print(f"ObjectRef/EvidenceRef fixture verification failed: {error}", file=sys.stderr)
        raise SystemExit(1)
    print("ObjectRef/EvidenceRef v1 fixtures and negative checks: PASS")
