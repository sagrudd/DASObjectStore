# Platform Probing Notes

Status: Draft  
Applies to: `dasobjectstore probe`, health collection, and enclosure discovery

## Intent

Platform probing provides best-effort observations about disks, filesystems,
transports, and enclosure topology.

Probe output is not authoritative pool metadata. Persistent pool metadata and
DASObjectStore disk identifiers must remain the source of truth once a disk is
adopted into a pool.

## USB DAS Identity Limits

USB DAS enclosures often sit behind bridge chips that can hide or rewrite device
identity.

Known limitations:

- hardware serial numbers may be missing, generic, unstable, or repeated across
  bays;
- model strings may describe the USB bridge rather than the HDD;
- bus paths may change when the enclosure is moved to another host or port;
- bay order may not be exposed by the operating system;
- a multi-bay enclosure may appear as independent USB storage devices with no
  reliable shared enclosure identifier;
- some bridges expose only one disk reliably when multiple disks are attached;
- hotplug and removable flags are hints, not proof that a device is safe to
  remove.

DASObjectStore should therefore identify disks using a composite identity:

- DASObjectStore-managed disk UUID once initialized;
- observed serial and model hints;
- size;
- partition and filesystem hints;
- USB topology hints;
- user-confirmed enclosure and bay labels where available.

## SMART Limits

SMART visibility through USB is inconsistent.

Expected constraints:

- some USB bridges do not pass SMART data through at all;
- some bridges require bridge-specific command modes;
- macOS may expose less SMART detail for USB-attached disks than Linux;
- NVMe, SATA, and USB-attached SATA disks can require different collection
  methods;
- SMART support may change with enclosure firmware or cable/port changes;
- a successful SMART read does not guarantee that future reads will keep working
  after moving the DAS to another host.

Health decisions must combine SMART data with other signals rather than relying
on SMART alone.

Command-output fixture expectations for platform parsers are documented in
[Platform Fixture Policy](platform-fixtures.md).

## Health Signal Policy

The health model should treat these signals as independent inputs:

- SMART attributes where available;
- IO errors;
- checksum failures;
- temperature signals;
- USB reset or disconnect events;
- benchmark drift;
- repeated command failures;
- user-confirmed physical replacement or retirement.

When SMART data is missing, DASObjectStore should report the missing signal
explicitly instead of treating the disk as healthy.

## Enclosure Grouping Policy

Enclosure grouping is best effort.

Automatic grouping may use USB topology paths, bridge hints, vendor/product
strings, and user-confirmed labels. The software should not assume that two
disks are in different enclosures unless the evidence is strong enough for the
selected store policy.

On Linux, the QNAP TL-D800C is treated as an explicitly recognized USB DAS
enclosure when udev exposes either direct QNAP TL-D800C device metadata or a
QNAP parent USB hub topology. The TL-D800C commonly presents each drive as an
individual block device behind ASMedia bridges while the QNAP identity appears
on parent USB hubs. DASObjectStore therefore maps TL-D800C disks to the
upstream QNAP hub path, so downstream branches of the same unit are reported as
one physical enclosure while other host USB ports remain separate.

Production object store creation requires managed HDDs to map to a supported,
identifiable DAS enclosure. Initially, that supported enclosure set is limited
to QNAP TL-D800C. If the probe cannot link prepared HDDs to the supported
enclosure family, `store create` fails before writing store registry state.

For protected stores that require enclosure-aware placement, uncertain topology
should be treated conservatively:

- prefer confirmed distinct enclosures;
- warn when only weak topology hints are available;
- avoid silently satisfying strict placement rules with uncertain evidence.

## User-Facing Behavior

Probe and health commands should make uncertainty visible.

They should distinguish:

- confirmed identity;
- observed hints;
- missing data;
- conflicting data;
- unsupported platform or bridge behavior.

`dasobjectstore health --connections` reports observed disk transport and warns
when USB-attached DAS performance may be limited by an unverified or slow link.
The warning is intentionally conservative because many platforms and USB bridge
chips do not expose negotiated link speed reliably.

When the same probe observes a better attached path, such as a Thunderbolt DAS
connection, connection health should point the user at that observed device and
topology path. If no better path is visible, the command should say so and
recommend trying a direct USB-C, USB4, or Thunderbolt host port without hubs or
fallback cables rather than pretending it can enumerate unused physical ports.

Risky operations such as disk retirement, forced import, or protected-store
placement should not depend on a single unstable hardware identifier.
