#!/usr/bin/env python3
"""Verify the immutable Terraform inputs for a formal remote package build."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any


PACKAGE_NAME = "dasobjectstore-remote"
PRODUCT_ID = "dasobjectstore"
SOURCE_LOCK_KEY = "dasobjectstore"
SHA256 = re.compile(r"sha256:[0-9a-f]{64}\Z")
REVISION = re.compile(r"[0-9a-f]{40}\Z")


def fail(message: str) -> None:
    print(f"formal remote release input: {message}", file=sys.stderr)
    raise SystemExit(1)


def require_file(value: str, label: str) -> Path:
    path = Path(value)
    if not path.is_file():
        fail(f"{label} is not a readable file: {path}")
    return path


def load_json(path: Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        fail(f"{label} is invalid: {error}")
    if not isinstance(value, dict):
        fail(f"{label} must contain a JSON object")
    return value


def value_at(mapping: dict[str, Any], label: str, *keys: str) -> str:
    current: Any = mapping
    for key in keys:
        if not isinstance(current, dict) or key not in current:
            fail(f"{label} is missing {'.'.join(keys)}")
        current = current[key]
    if not isinstance(current, str) or not current:
        fail(f"{label} has an invalid {'.'.join(keys)}")
    return current


def lock_authority(mapping: dict[str, Any], label: str) -> tuple[str, str, str]:
    lockset_id = value_at(mapping, label, "lockset_id")
    content_digest = value_at(mapping, label, "content_digest")
    registry_digest = value_at(mapping, label, "registry_digest")
    if not SHA256.fullmatch(content_digest):
        fail(f"{label} has an invalid content_digest")
    if not SHA256.fullmatch(registry_digest):
        fail(f"{label} has an invalid registry_digest")
    if any(character.isspace() for character in lockset_id):
        fail(f"{label} has an invalid lockset_id")
    return lockset_id, content_digest, registry_digest


def lockset_authority(mapping: dict[str, Any]) -> tuple[str, str, str]:
    lockset_id = value_at(mapping, "TERRAFORM_SUCCESSOR_LOCKSET", "source_lockset_id")
    content_digest = value_at(mapping, "TERRAFORM_SUCCESSOR_LOCKSET", "content_digest")
    registry_digest = value_at(mapping, "TERRAFORM_SUCCESSOR_LOCKSET", "registry_digest")
    if not SHA256.fullmatch(content_digest):
        fail("TERRAFORM_SUCCESSOR_LOCKSET has an invalid content_digest")
    if not SHA256.fullmatch(registry_digest):
        fail("TERRAFORM_SUCCESSOR_LOCKSET has an invalid registry_digest")
    if any(character.isspace() for character in lockset_id):
        fail("TERRAFORM_SUCCESSOR_LOCKSET has an invalid source_lockset_id")
    return lockset_id, content_digest, registry_digest


def git_head(repo_root: Path) -> str:
    result = subprocess.run(
        ["git", "-C", str(repo_root), "rev-parse", "HEAD"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    head = result.stdout.strip()
    if result.returncode or not REVISION.fullmatch(head):
        fail("repository HEAD is not an exact Git revision")
    status = subprocess.run(
        ["git", "-C", str(repo_root), "status", "--porcelain", "--untracked-files=all"],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if status.returncode or status.stdout:
        fail("repository must be clean for a formal remote package build")
    return head


def cargo_remote_version(repo_root: Path) -> str:
    result = subprocess.run(
        [
            "cargo",
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            str(repo_root / "Cargo.toml"),
        ],
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode:
        fail("could not read the Cargo package metadata")
    try:
        metadata = json.loads(result.stdout)
    except json.JSONDecodeError:
        fail("Cargo metadata was not valid JSON")
    matches = [
        package
        for package in metadata.get("packages", [])
        if package.get("name") == PACKAGE_NAME and isinstance(package.get("version"), str)
    ]
    if len(matches) != 1:
        fail(f"Cargo metadata must contain exactly one {PACKAGE_NAME} package")
    return matches[0]["version"]


def expected_authority(actual: tuple[str, str, str], expected: tuple[str, str, str], label: str) -> None:
    if actual != expected:
        fail(
            f"{label} authority does not match lockset "
            f"(expected {expected[0]} {expected[1]} {expected[2]})"
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", required=True)
    parser.add_argument("--lockset", required=True)
    parser.add_argument("--catalogue", required=True)
    parser.add_argument("--sources-lock", required=True)
    parser.add_argument("--package-version", required=True)
    args = parser.parse_args()

    repo_root = require_file(str(Path(args.repo_root) / "Cargo.toml"), "repository Cargo.toml").parent
    lockset_path = require_file(args.lockset, "TERRAFORM_SUCCESSOR_LOCKSET")
    catalogue_path = require_file(args.catalogue, "TERRAFORM_CATALOGUE")
    sources_path = require_file(args.sources_lock, "TERRAFORM_SOURCES_LOCK")
    head = git_head(repo_root)
    cargo_version = cargo_remote_version(repo_root)
    if cargo_version != args.package_version:
        fail(f"Cargo {PACKAGE_NAME} version {cargo_version} does not match requested {args.package_version}")

    try:
        lockset = tomllib.loads(lockset_path.read_text())
    except (OSError, tomllib.TOMLDecodeError) as error:
        fail(f"TERRAFORM_SUCCESSOR_LOCKSET is invalid: {error}")
    if not isinstance(lockset, dict):
        fail("TERRAFORM_SUCCESSOR_LOCKSET must contain a TOML table")
    if lockset.get("schema_version") != "mnemosyne.terraform.kanon-lockset-projection.v1":
        fail("TERRAFORM_SUCCESSOR_LOCKSET is not a Kanon lockset projection")
    authority = lockset_authority(lockset)
    components = lockset.get("components")
    if not isinstance(components, list):
        fail("TERRAFORM_SUCCESSOR_LOCKSET is missing components")
    matching = [
        component
        for component in components
        if isinstance(component, dict) and component.get("product_id") == PACKAGE_NAME
    ]
    if len(matching) != 1:
        fail(f"TERRAFORM_SUCCESSOR_LOCKSET must contain exactly one {PACKAGE_NAME} component")
    component = matching[0]
    if component.get("version") != args.package_version:
        fail("lockset component version does not match the remote package version")
    if component.get("source_revision") != head:
        fail("lockset component source_revision does not match repository HEAD")
    if not REVISION.fullmatch(head):
        fail("lockset component source_revision is not an exact Git revision")
    provenance = component.get("version_provenance")
    if (
        not isinstance(provenance, dict)
        or provenance.get("package_name") != PACKAGE_NAME
        or provenance.get("source_revision") != head
    ):
        fail("lockset component version_provenance.package_name is not dasobjectstore-remote")

    catalogue = load_json(catalogue_path, "TERRAFORM_CATALOGUE")
    catalogue_authority = catalogue.get("compatibility_authority")
    if not isinstance(catalogue_authority, dict):
        fail("TERRAFORM_CATALOGUE is missing compatibility_authority")
    if catalogue.get("schema") != "mnemosyne.terraform.component-catalogue.v1":
        fail("TERRAFORM_CATALOGUE is not a Terraform component catalogue")
    if catalogue_authority.get("kind") != "kanon_lockset_projection":
        fail("TERRAFORM_CATALOGUE compatibility authority is not a Kanon lockset projection")
    expected_authority(lock_authority(catalogue_authority, "TERRAFORM_CATALOGUE"), authority, "catalogue")
    catalogue_components = catalogue.get("components")
    if not isinstance(catalogue_components, dict):
        fail("TERRAFORM_CATALOGUE is missing components")
    catalogue_component = catalogue_components.get(PACKAGE_NAME)
    if not isinstance(catalogue_component, dict):
        fail(f"TERRAFORM_CATALOGUE is missing {PACKAGE_NAME}")
    if catalogue_component.get("source_lock_key") != SOURCE_LOCK_KEY:
        fail("catalogue source_lock_key is not dasobjectstore")
    package = catalogue_component.get("package")
    names = package.get("names") if isinstance(package, dict) else None
    package_names = (
        [name for values in names.values() for name in values]
        if isinstance(names, dict) and all(isinstance(values, list) for values in names.values())
        else []
    )
    if PACKAGE_NAME not in package_names:
        fail("catalogue package names do not include dasobjectstore-remote")

    sources = load_json(sources_path, "TERRAFORM_SOURCES_LOCK")
    sources_authority = sources.get("authority")
    if not isinstance(sources_authority, dict):
        fail("TERRAFORM_SOURCES_LOCK is missing authority")
    if sources_authority.get("schema") != "mnemosyne.terraform.sources-lock-authority.v1":
        fail("TERRAFORM_SOURCES_LOCK authority is not a Terraform sources lock")
    expected_authority(lock_authority(sources_authority, "TERRAFORM_SOURCES_LOCK"), authority, "sources lock")
    sources_components = sources.get("components")
    source = sources_components.get(SOURCE_LOCK_KEY) if isinstance(sources_components, dict) else None
    if not isinstance(source, dict) or source.get("revision") != head:
        fail("sources lock dasobjectstore revision does not match repository HEAD")

    manifest = load_json(
        require_file(str(repo_root / "product-manifest.json"), "product manifest"),
        "product manifest",
    )
    product = manifest.get("product")
    if not isinstance(product, dict) or product.get("id") != PRODUCT_ID:
        fail("product manifest product.id is not dasobjectstore")
    if product.get("version") != args.package_version:
        fail("product manifest version does not match the remote package version")

    print(" ".join((*authority, head)))


if __name__ == "__main__":
    main()
