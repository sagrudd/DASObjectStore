# Actual Garage fixture successor — execution held

Source-only qualification proposal for issue212 / PR213, within0.186.0.
No production activation, live NUC resource, new protocol or formal companion.
Root reviewed this scope and approved public-only extraction and strict
fixture dispatch; actual execution remains held for the frozen fixture review.
Current source checkpoint implements command-dispatch negatives and the ignored
actual admission/finite-retention entry. Linux compilation, guest orchestration,
restart checks and feeding these receipts into the existing TLS fixture remain
subsequent work; the broader matrix below is not claimed implemented or passed.

## Existing artifact selection

The repository's `DEFAULT_GARAGE_IMAGE`, inert custody Compose asset and
benchmark select `dxflrs/garage:v2.3.0`. A version tag alone is not immutable.
Read-only Docker Hub tag metadata on2026-09-08 reports:

- index `sha256:866bd13ed2038ba7e7190e840482bc27234c4afaf77be8cfa439ae088c1e4690`;
- linux/arm64 child `sha256:2d3f94a89a8a02dc49fa75594d6df67ed9c6ffe08fe55ed023d0c9776f71a9bd`;
- compressed size27085033bytes.

Source: `https://hub.docker.com/v2/repositories/dxflrs/garage/tags/v2.3.0`.
Before use, retain registry manifest/config bytes and verify their digests,
platform and layer closure. Extract only the public `/garage` executable from
the pinned image without starting it; hash and inspect ELF architecture/linkage.
Publisher registry provenance is not an independently signed release claim.
No tag re-resolution fallback, alternate version or unreviewed library copying.

## Proposed isolated scope

Reuse the pristine reviewed5e516cee public VM input and modern QEMU/KVM bounds:
fresh guest disk, no virtual NIC, host networknone, no host mounts, UID1000,
cap-dropALL, no-new-privileges, only /dev/kvm rw, 4GiB/2CPU/PIDs256, independent
900s controller. Actual Garage runs only inside the disposable guest, under
a separate unprivileged service identity with a private root and loopback
S3/RPC. Disable public web/admin listeners unless a required existing operation
actually needs them. No host Docker socket is exposed to guest or reader.
Generated RPC/access secrets remain guest-only, never console/seed/evidence.
Do not export any booted image or generated credential layer.

Single-node fixture readiness must prove an applied node layout and successful
actual CLI query before admission. A fresh fixture bucket is created through
`GarageCustodyProvisioner`, never synthesized fresh-bucket proof. The existing
`CustodyServiceController` supplies catalogue claims/admission and
`retain_custody_inventory` supplies finite validation, one-use handoffs, actual
conditional PUT, metadata verification, readback and receipts.

Use two distinct small synthetic byte objects (each <=4096bytes), chosen solely
for finite-batch regression. This is neither an assertion of two formal backend
objects nor a mapping of the17 Jenkins consumer files. Retention-until is a
fixture-selected future timestamp and permanent legal hold follows the existing
sealed local-trusted-administrator overlay. Garage2.3 has no native Object Lock;
never claim WORM/COMPLIANCE or protection against its administrator. No shortening
or object deletion is part of the test; dispose of the entire synthetic guest
after evidence selection, not through a production retention override.

## Exact adapter composition to review

The provisioner currently builds `docker compose ... exec -T SERVICE /garage`
arguments. A test-only `ServiceCommandRunner` may accept only that complete
fixed fixture wrapper and forward its remaining argv to the actual pinned
Garage executable/config. No shell, generic passthrough, fabricated stdout,
error translation or replacement freshness/grant verifier is permitted. AWS
calls use the genuine existing process runner and signed Fedora AWS closure.
This proves the actual CLI/S3 adapters, not Docker Compose containment. If
review requires actual Compose itself, stop and select its coherent guest
dependency closure instead of silently changing this attribution.

## Connected checks

1. Actual absent bucket -> exact writer-W/reader-R grants -> admitted catalogue;
   existing bucket/claim denies without adoption, including reconstructed state.
2. Whole inventory mismatch before handoff/PUT; valid finite batch returns two
   receipts matching actual ledger and backend bytes/metadata. No mock backend.
3. New process/resolver cannot reuse consumed writer/reader handoffs. Failed
   retention preserves prior objects, catalogue, ledger and claims.
4. Feed the actual ledger/receipts into existing protected publication and
   systemd continuation; actual TLS client/journal reads exact bytes with AWS.
5. Direct reader write and writer read attempts measure actual denied grants;
   backend administrator capabilities remain outside overlay protection.
6. Stop/restart the same Garage guest service without new admission; readback
   must preserve bytes, receipt/ledger identity and journal non-replay.

Each boundary has a fixed non-secret marker and immutable-input hashes; retain
no credential-bearing argv, raw admin output or signed request headers.
Readiness, syscall/process deadlines, partial failure and cleanup are explicit.
No fixture execution until root approves immutable artifact and isolation.

## Still separate

Physical off-NUC verifier provenance and independently measured endpoints are
not established by two guest UIDs. Real companion must select target, finite
storage mapping, duration, hold, manager/service identities and protected key
delivery. Formal S4 additionally requires exact retained consumer bytes and
the independently admitted companion/one-use recorder boundary. These are not
new source-code gates, and this fixture cannot grant their execution authority.
