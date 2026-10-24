# INT-07 inherited module-size baseline

This pending 0.187.2 source transaction records fourteen oversized production
files inherited byte-for-byte from normally merged revision
`35d60d1445b98a4fc9c466b8bfcdc28e76d80597`. The failed native governance
operation `f667a076` remains a failure. Its four preceding scripts passed;
the final size guard identified these files. This is source organization debt,
not a runtime, security, dependency, CI-authority or release exception.

The existing guard expressly permits a reviewed temporary baseline. Engineering
Standard AGENTS.md requires downstream exceptions to be explicit, narrow,
time-bounded and recorded. Independent engineering review is required before
adoption. The existing five legacy exceptions are unchanged. The 1,000-line
limit and its production-line counting remain unchanged.

The exact fourteen full-file hashes, current line ceilings and remediation
records are in `rust-module-size-baselines.json`. Every record expires **before
31 October 2026 UTC**. Any changed bytes, growth, duplicate/malformed record,
expiry, or unlisted oversized file fails. A smaller file needs no exception.
The check does not describe oversized files as meeting the normal size limit.
The baseline checker requires Python 3, already used by this qualification.

Accountable implementation owner: the existing INT-07 delivery owner (Codex in
this chat); project owner: Stephen. Before expiry, split each responsibility
below and remove its record once the normal guard and applicable compiler,
strict and behavioral checks pass. No further extension is implicit.

| Module | Responsibility to extract |
| --- | --- |
| CLI run | CLI error conversion and display |
| Core application auth | Token exchange and claims |
| Core S6 custody | Contract types and authority validation |
| Daemon client | Response names and unexpected-response handling |
| Daemon main | Workspace startup workers |
| Runtime custody garage | Systemd credential handoff |
| Runtime remote sessions | Error representation and conversion |
| Runtime service | Garage service controller |
| Request handler | Authority admission versus dispatch |
| Provider stream | Ingress commit/readback versus reader transport |
| GUI auth routes | Preverified host handlers |
| Object-service custody | Types/policy, off-NUC operations, ledger writes/storage; separate test module |
| Custody attestation | Validation and codec |
| Bootstrap observer | Trusted-descriptor public reads |

Preserve API visibility, serialization, authority checks and unsafe-code
contracts during those later refactors. This baseline avoids introducing
unrelated production changes into the current package qualification. Native
consumer verification must be rerun against the adopted exact source; local
checker tests alone cannot establish governance, installation or full-flow
acceptance. Keep the failed original and record the new operation separately.
