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

Risky operations such as disk retirement, forced import, or protected-store
placement should not depend on a single unstable hardware identifier.
