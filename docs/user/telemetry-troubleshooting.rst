Restoring Home Telemetry
========================

This runbook is for an operator who sees unavailable, stale, or warming-up
telemetry on the Web Home page. Home is the normal inspection surface. The
authenticated ``appliance_telemetry`` API is the equivalent interface for API
clients; direct state-file reads below are read-only diagnostics, not a second
control plane.

The packaged-daemon checks in this page require a Linux appliance. They are
included so an operator can collect consistent evidence when one is available;
they are not evidence that the current macOS checkout has been accepted on an
appliance. Local validation uses the fixture matrix described at the end of
this page.

Classify the Home state first
-----------------------------

* ``first_sample_warmup`` is expected after daemon startup or a disk counter
  reset. Wait for the next configured cadence before treating it as a fault.
* ``device_missing``, ``permission_denied``, ``collector_unavailable``,
  ``unsupported_platform``, and ``counter_reset`` are explicit collection
  diagnostics. They are not zero throughput.
* A valid sample with zero rates is a healthy idle interval. Null rates or a
  missing reason mean that no trustworthy rate was available.
* A stale Home snapshot means the Web refresh could not obtain a newer daemon
  result. Follow the service and state-file checks below before assuming that
  the appliance is idle.

Check the daemon loop and sample age
------------------------------------

On a packaged Linux appliance, inspect the daemon and validate configuration
without changing storage state::

   sudo dasobjectstored --config /etc/dasobjectstore/daemon.json --check-config
   sudo systemctl status dasobjectstored.service
   sudo systemctl show dasobjectstored.service --property=ActiveState,SubState,ExecMainStartTimestamp
   sudo journalctl -u dasobjectstored.service --since "15 min ago" \
     | grep -E 'appliance telemetry (collection failed|write failed)'

The supported cadence is 30 seconds for normal operation or 6 seconds for
short diagnostics. After correcting configuration through the supported
management path, wait at least two cadences and compare the state-file
timestamp and modification time. Do not treat a running systemd unit alone as
proof that a sample was collected.

Inspect the state file read-only
--------------------------------

The daemon-owned state path is
``/var/lib/dasobjectstore/telemetry/appliance-telemetry.v1.json``. Verify its
owner, mode, JSON syntax, and newest ``generated_at_utc`` value::

   sudo stat -c '%U:%G %a %n' /var/lib/dasobjectstore/telemetry
   sudo stat -c '%U:%G %a %n' /var/lib/dasobjectstore/telemetry/appliance-telemetry.v1.json
   sudo python3 -m json.tool /var/lib/dasobjectstore/telemetry/appliance-telemetry.v1.json >/dev/null
   sudo grep -n '"generated_at_utc"' /var/lib/dasobjectstore/telemetry/appliance-telemetry.v1.json | head -1

Do not edit, truncate, or delete this file while the daemon is running. If it
is corrupt, preserve the evidence and use the documented stop/move/start reset
procedure in :doc:`service-boundary` (the reset discards chart history only;
it does not repair collection failures).

Check disk identity and kernel counters
---------------------------------------

For each affected managed HDD, inspect the marker and mount/device mapping::

   sudo cat /srv/dasobjectstore/hdd/<disk-id>/.dasobjectstore/device.env
   findmnt -T /srv/dasobjectstore/hdd/<disk-id>
   readlink -f /dev/disk/by-id/<stable-alias>
   grep -E '(^| )<device-name>( |$)' /proc/diskstats

Markers must be read as evidence, not edited in place. Confirm ``role=hdd:<disk-id>``
and compare ``device``, optional ``diskstats_device``, ``enclosure_id``, and
``bay_label`` with ``findmnt``, sysfs identity, and the matching
``/proc/diskstats`` row. Partitions, stable USB ``by-id`` aliases, and
device-mapper ``by-path`` aliases are valid mappings; an unresolved mapping is
reported as ``device_missing`` rather than converted to zeroes. Correct a
mapping only through the supported disk-management workflow.

Safe escalation evidence
------------------------

Capture the Home warning, selected telemetry window, daemon service status,
recent journal diagnostics, state-file timestamp/permissions, each affected
marker, ``findmnt``/sysfs identity, and the relevant ``/proc/diskstats`` row.
After a supported repair, verify that Home changes from unavailable or stale
to a fresh sample after two cadences. Never delete managed roots, fabricate
telemetry values, or use a telemetry reset as a substitute for fixing service,
permission, or device-mapping errors.

Local macOS validation
----------------------

The offline regression matrix runs without an appliance and uses only generated
fixtures under ``/Users/stephen/.dasobjectstore-codex-validation``. Run::

   cargo test -p dasobjectstore-daemon --test appliance_telemetry

It covers direct SATA names, partitions, stable USB aliases, device-mapper
aliases, missing devices, first-sample warm-up, and subsequent non-zero rates.
Packaged daemon-loop and authoritative enclosure-topology acceptance remain
appliance-dependent gates tracked in ``ROADMAP.md`` and ``TODO.md``.
