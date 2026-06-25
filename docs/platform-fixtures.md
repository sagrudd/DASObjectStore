# Platform Fixture Policy

Status: Draft
Scope: command-output fixtures for platform probing and health parsing

## Intent

Platform command output is a compatibility-sensitive parsing surface.
DASObjectStore must keep representative command output fixtures beside parser
tests whenever a platform parser is introduced or materially changed.

The goal is to make macOS and Linux development reliable without requiring DAS
hardware to exercise every parser path.

## Current Fixture Coverage

| Parser | Command shape | Fixture | Test expectation |
| --- | --- | --- | --- |
| Linux disk inventory | `lsblk --json ...` | `crates/dasobjectstore-platform/fixtures/linux/lsblk-usb-das.json` | Parses USB DAS disk inventory and rejects invalid JSON. |
| Linux SMART health | `smartctl --json --health --attributes <device>` | `crates/dasobjectstore-platform/fixtures/linux/smartctl-sata-warning.json` | Parses warning signals and rejects invalid JSON. |
| macOS disk inventory | `diskutil list -plist` | `crates/dasobjectstore-platform/fixtures/macos/diskutil-list-usb-das.plist` | Parses removable direct-attached disks and rejects invalid plist. |
| macOS SMART health | `diskutil info -plist <device>` | `crates/dasobjectstore-platform/fixtures/macos/diskutil-info-smart-failing.plist` | Parses failing SMART status. |
| macOS unsupported SMART | `diskutil info -plist <device>` | `crates/dasobjectstore-platform/fixtures/macos/diskutil-info-smart-unsupported.plist` | Preserves unsupported SMART as an explicit missing signal. |

## Rules For New Parsers

When adding a parser for external command output:

1. Commit at least one representative success fixture.
2. Add a parser test that uses the fixture through `include_str!` or
   `include_bytes!`.
3. Add a negative test for malformed or unsupported input.
4. Assert the command name and stable argument shape separately from parsing.
5. Document known platform or bridge limitations in
   [Platform Probing Notes](probing.md) when the fixture reveals user-visible
   uncertainty.

Fixtures should be small, redacted, and representative. They should preserve
field names and value shapes that the parser depends on, but they must not
include user secrets, hostnames, or private serial numbers.

## Review Checklist

Before merging a platform parser change:

- the fixture path is committed under `crates/dasobjectstore-platform/fixtures`;
- tests run on macOS without attached DAS hardware;
- parser errors name the source command clearly;
- unsupported hardware behavior is represented as explicit missing or degraded
  signal state rather than silent health.
