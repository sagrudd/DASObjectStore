# Direct S3 ingress threat and failure analysis

Status: Required release evidence for `direct_gateway`
Related decision: [DASObjectStore-owned direct S3 ingress](direct-s3-ingress-adr.md)

## Trust boundaries

The remote S3 client and network are untrusted.  The standalone gateway is a
protocol and authentication boundary, not a storage authority.  The daemon,
its Unix socket, managed credential registry, profile registry, capacity
ledger, catalogue, ingress journal, and managed mounts form the trusted
appliance boundary.  Garage is a compatible legacy provider and recovery
source; it is not authoritative for direct-object placement.

Secret access keys stay in the daemon-owned credential registry.  The gateway
may return only the public access-key identity and resolved store/bucket.  Logs,
errors, audit records, and status payloads must never contain a secret, signing
key, Authorization header, or complete presigned URL.

## Threats and controls

| Threat | Required control | Acceptance evidence |
|---|---|---|
| Cross-store or cross-bucket write | Resolve the access key to one managed credential and require its exact bucket; derive the store server-side | A valid signature for store A is rejected against bucket/store B |
| Client-selected managed path or placement | S3 requests contain only bucket/key; daemon resolves profile, store-private root, and placement policy | Traversal, encoded separator, absolute path, store/path header, and ambiguous normalization tests fail closed |
| Request smuggling or canonicalization ambiguity | One bounded HTTP parser; reject duplicate signed headers, malformed percent encoding, encoded path separators, and ambiguous canonical query/header forms | SigV4 canonical-request fixtures and duplicate-header tests |
| Payload substitution or truncation | Require a fixed hexadecimal `x-amz-content-sha256` for PUT, compute SHA-256 and count in flight, and compare both before publication | Length/checksum mismatch and disconnect tests leave no visible object |
| Credential disclosure | Mode `0600` registry, no secret-bearing diagnostics, least-privilege store-scoped keys, bounded rotation audit | Package mode test and log/audit redaction test |
| Replay or conflicting retry | Deterministic operation identity plus journal; exact size/checksum replay is idempotent, differing content is a conflict | Duplicate completion returns the same object; conflicting retry never overwrites it |
| Reservation exhaustion | Logical and physical capacity admission before accepting payload; bounded global upload permits and streaming backpressure | Over-capacity and concurrency tests reject or queue without leaking reservations |
| Multipart abuse | Bound part size/count, total retained bytes, concurrent uploads, idle lifetime, and completion manifest; retain only explicitly resumable state | Invalid ordering/duplicate part/oversize/expiry/abort/restart tests |
| SQLite denial of service | Do not hold write transactions during transfer; bounded busy timeout/retry with jitter around the short commit | Lock-contention test transfers once and either commits or returns a retryable failure |
| Symlink or managed-root escape | Store-private root, no-follow/symlink checks, restrictive modes, atomic rename on the same filesystem | Symlink fixture is rejected before a file is opened or published |
| Network observation or replay | Production exposure requires trusted-network placement or TLS termination; SigV4 authenticates requests but plaintext HTTP does not provide confidentiality | Network policy/TLS termination recorded in appliance evidence |

The SigV4 verifier must enforce a bounded clock-skew/replay window before the
gateway is exposed beyond a trusted appliance network.  A matching credential
scope date alone is not a freshness policy.  Presigned-query authentication,
streaming SigV4 chunk signatures, and `UNSIGNED-PAYLOAD` writes are unsupported
unless independently implemented and tested; clients must use a signed fixed
payload digest for direct PUT.

## Failure modes and recovery

| Failure point | Externally visible result | Durable recovery rule |
|---|---|---|
| Disconnect before complete body | No S3 success and no catalogue object | Mark non-resumable PUT aborted; release reservations; GC only the proven temporary file |
| Multipart interruption | No completed object | Retain a bounded resumable manifest and verified parts until resume, explicit abort, or expiry |
| Length/checksum mismatch | S3 error; object remains invisible | Record rejection, release capacity, reclaim non-resumable data |
| SSD fsync/rename failure | No S3 success | Journal remains pre-published; recovery must not synthesize catalogue visibility |
| Daemon crash while receiving | Connection fails | Restart reads journal, rejects partial ordinary PUT, and retains only valid resumable multipart state |
| Daemon crash after file publication but before metadata commit | Client sees failure or uncertainty | Replay the same journal identity; exact retry finishes publication without retransferring bytes |
| Catalogue commit contention/failure | Retryable S3 error, never false success | Bounded retry around metadata only; do not replay the network payload |
| Crash after catalogue commit but before HTTP response | Client sees uncertainty | Exact retry returns the already accepted object and its stable digest |
| Destage worker unavailable under `AfterSsdIngest` | Success only if the durable destage job was committed | Worker resumes from durable queue after restart |
| HDD placement failure under `AfterHddPlacement` | No success until required verified copy exists | Keep accepted SSD state and durable retry/failure status; do not falsely report policy completion |
| Garage unavailable in direct mode | Direct managed PUT/read operations continue if their daemon authorities are healthy; legacy-provider operations fail explicitly | Do not redirect a direct write into Garage as an implicit fallback |
| Gateway unavailable | S3 endpoint unavailable | Roll back listener ownership to Garage only through the documented mode change; never run both on the same port |
| GC crash | No accepted placement may be removed | Idempotent, journal-aware sweep; re-evaluate catalogue/placement proof after restart |

## Required audit vocabulary

The authority should record initiation, admission wait/rejection, receive
completion, verification, publication, acknowledgement, exact replay,
conflicting retry, cancellation, multipart resume/abort/expiry, recovery, and
GC reclamation.  Each event should carry operation ID, store ID, bucket, key
digest or safely escaped key, byte count, policy, state, and reason.  It must
not carry credentials or signing material.

## Stop conditions

Do not enable `direct_gateway`, and roll back if already enabled, when any of
the following is true:

- the credential registry maps one access key ambiguously;
- Garage still owns the public port or the private upstream is externally
  exposed;
- daemon publication, capacity, or journal recovery tests fail;
- ordinary PUT or multipart acknowledgement can precede the required policy;
- reconciliation recopies a direct object;
- GC cannot explain why each candidate is unreferenced;
- control-plane health becomes unavailable under the configured upload budget;
- the appliance cannot preserve the previous configuration and provider data
  for rollback.
