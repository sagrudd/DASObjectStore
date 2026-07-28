#!/usr/bin/env python3
"""Non-destructive EPIC C inspection and evidence validation.

Load orchestration remains in the adjacent shell wrapper. This helper keeps
percentile calculation and the commit-bound evidence envelope deterministic.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import os
import pathlib
import shutil
import subprocess
import sys
import urllib.error
import urllib.request

SCHEMA_VERSION = "dasobjectstore.epic-c-appliance-evidence.v1"
ONE_TIB = 1 << 40


def command(*args: str) -> str:
    try:
        return subprocess.run(
            args, check=False, capture_output=True, text=True, timeout=10
        ).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return ""


def percentile(values: list[float], quantile: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    return round(ordered[max(0, math.ceil(len(ordered) * quantile) - 1)], 3)


def sample_url(
    url: str, count: int, cafile: str | None, cookie_file: str | None = None
) -> dict:
    import http.cookiejar
    import ssl
    import time

    context = ssl.create_default_context(cafile=cafile)
    handlers: list = [urllib.request.HTTPSHandler(context=context)]
    if cookie_file:
        jar = http.cookiejar.MozillaCookieJar(cookie_file)
        jar.load(ignore_discard=True, ignore_expires=True)
        handlers.append(urllib.request.HTTPCookieProcessor(jar))
    opener = urllib.request.build_opener(*handlers)
    timings: list[float] = []
    successes = 0
    statuses: list[int] = []
    for _ in range(count):
        started = time.monotonic()
        try:
            with opener.open(url, timeout=3) as response:
                response.read()
                status = response.status
        except urllib.error.HTTPError as error:
            error.read()
            status = error.code
        except (OSError, urllib.error.URLError):
            status = 0
        timings.append((time.monotonic() - started) * 1000)
        statuses.append(status)
        successes += int(200 <= status < 300)
    return {
        "samples": count,
        "http_successes": successes,
        "p50_ms": percentile(timings, 0.50),
        "p95_ms": percentile(timings, 0.95),
        "p99_ms": percentile(timings, 0.99),
    }


def read_psi() -> dict:
    result = {}
    for resource in ("cpu", "io", "memory"):
        path = pathlib.Path("/proc/pressure") / resource
        result[resource] = path.read_text().strip() if path.exists() else "unavailable"
    return result


def psi_some_avg10(snapshot: dict, resource: str) -> float:
    lines = str(snapshot.get(resource, "")).splitlines()
    if not lines:
        return 0.0
    for field in lines[0].split():
        if field.startswith("avg10="):
            try:
                return float(field.split("=", 1)[1])
            except ValueError:
                return 0.0
    return 0.0


def device_queues() -> dict:
    result = {}
    for path in pathlib.Path("/sys/class/block").glob("*/stat"):
        fields = path.read_text().split()
        if len(fields) >= 11:
            result[path.parent.name] = {
                "io_in_progress": int(fields[8]),
                "io_time_ms": int(fields[9]),
                "weighted_io_time_ms": int(fields[10]),
            }
    return result


def staging_bytes() -> int:
    root = pathlib.Path(os.environ.get("DASOBJECTSTORE_ACCEPTANCE_STAGING_ROOT", "/srv/dasobjectstore/ssd"))
    if not root.exists():
        return 0
    output = command("du", "-sk", str(root))
    try:
        return int(output.split()[0]) * 1024
    except (IndexError, ValueError):
        return 0


def acceptance_record(args: argparse.Namespace, mode: str) -> dict:
    source = command("git", "-C", args.repo, "rev-parse", "HEAD") or "unavailable"
    package = command("dpkg-query", "-W", "-f=${Version}", "dasobjectstore")
    if not package:
        package = command("rpm", "-q", "--qf", "%{VERSION}-%{RELEASE}", "dasobjectstore")
    services = {
        name: command("systemctl", "is-active", name) or "unavailable"
        for name in ("dasobjectstored.service", "dasobjectstore-server.service")
    }
    live = sample_url(
        f"{args.base_url}/api/v1/liveness",
        args.samples,
        args.cafile,
        getattr(args, "cookie_file", None),
    )
    dashboard = sample_url(
        f"{args.base_url}/api/v1/dashboard/status",
        args.samples,
        args.cafile,
        getattr(args, "cookie_file", None),
    )
    mounts_raw = command("findmnt", "--json", "--target", "/srv/dasobjectstore")
    try:
        mounts = json.loads(mounts_raw) if mounts_raw else {}
    except json.JSONDecodeError:
        mounts = {"raw": mounts_raw}
    before = staging_bytes()
    return {
        "schema_version": SCHEMA_VERSION,
        "mode": mode,
        "result": "inspected" if mode == "inspect" else "incomplete",
        "captured_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "source": {"identity": source, "dirty": bool(command("git", "-C", args.repo, "status", "--porcelain"))},
        "package": {
            "identity": package or "unavailable",
            "cli": command("dasobjectstore", "--version") or "unavailable",
            "daemon": command("dasobjectstored", "--version") or "unavailable",
        },
        "services": services,
        "safety": {
            "store_id": "CODEX",
            "generated_bytes": 0,
            "maximum_total_bytes": ONE_TIB,
            "customer_or_project_data_used": False,
            "quiescent_gate": False,
            "explicit_confirmation": False,
        },
        "thresholds": {
            "liveness_p95_ms": args.liveness_p95_ms,
            "liveness_p99_ms": args.liveness_p99_ms,
            "dashboard_p95_ms": args.dashboard_p95_ms,
            "dashboard_p99_ms": args.dashboard_p99_ms,
            "accept_queue_max": args.accept_queue_max,
            "cpu_psi_some_avg10": args.cpu_psi_some_avg10,
            "io_psi_some_avg10": args.io_psi_some_avg10,
            "memory_psi_some_avg10": args.memory_psi_some_avg10,
            "device_io_in_progress": args.device_io_in_progress,
            "cancellation_ms": args.cancellation_ms,
            "staging_recovery_bytes": args.staging_recovery_bytes,
        },
        "https": {"liveness": live, "dashboard": dashboard},
        "host": {
            "accept_queue_max": 0,
            "psi": read_psi(),
            "device_queue": device_queues(),
            "device_queue_max": 0,
        },
        "telemetry": {
            "mounted_devices": mounts,
            "lsblk": command("lsblk", "--json", "-o", "NAME,KNAME,TYPE,SIZE,FSTYPE,MOUNTPOINTS,PKNAME"),
            "mapping_status": "inspect_only",
        },
        "staging": {
            "before_bytes": before,
            "peak_bytes": before,
            "after_bytes": before,
            "recovered_bytes": 0,
        },
        "cancellation": None,
        "failures": [],
    }


def write_evidence(path: str, evidence: dict) -> None:
    pathlib.Path(path).write_text(json.dumps(evidence, indent=2) + "\n")
    os.chmod(path, 0o600)


def inspect(args: argparse.Namespace) -> int:
    evidence = acceptance_record(args, "inspect")
    write_evidence(args.output, evidence)
    return 0


def recursive_numbers(value, names: set[str]) -> list[int]:
    found: list[int] = []
    if isinstance(value, dict):
        for key, item in value.items():
            if key in names and isinstance(item, int):
                found.append(item)
            found.extend(recursive_numbers(item, names))
    elif isinstance(value, list):
        for item in value:
            found.extend(recursive_numbers(item, names))
    return found


def codex_used_bytes() -> int:
    result = subprocess.run(
        ["dasobjectstore", "store", "capacity", "CODEX", "--json"],
        check=False,
        capture_output=True,
        text=True,
        timeout=30,
    )
    if result.returncode:
        raise RuntimeError("CODEX capacity inspection failed")
    try:
        record = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError("CODEX capacity inspection returned invalid JSON") from error
    values = recursive_numbers(record, {"logical_used_bytes", "used_bytes"})
    if not values:
        raise RuntimeError("CODEX capacity did not report authoritative used bytes")
    return max(values)


def accept_queue(port: int) -> int:
    output = command("ss", "-lntH", f"sport = :{port}")
    maximum = 0
    for line in output.splitlines():
        fields = line.split()
        if len(fields) >= 2:
            try:
                maximum = max(maximum, int(fields[1]))
            except ValueError:
                pass
    return maximum


def generate_random_file(path: pathlib.Path, size: int) -> None:
    chunk = 1024 * 1024
    with path.open("wb", buffering=0) as handle:
        remaining = size
        while remaining:
            payload = os.urandom(min(chunk, remaining))
            handle.write(payload)
            remaining -= len(payload)
        os.fsync(handle.fileno())


def generate_random_workload(root: pathlib.Path, total_size: int) -> None:
    # Multiple independent objects exercise configured source/SSD/HDD worker
    # concurrency; the total remains governed by the single CODEX byte gate.
    shard_count = min(16, max(4, math.ceil(total_size / (256 * 1024 * 1024))))
    base, remainder = divmod(total_size, shard_count)
    for index in range(shard_count):
        generate_random_file(
            root / f"codex-random-{index:02d}.bin",
            base + int(index < remainder),
        )


def load(args: argparse.Namespace) -> int:
    import time

    evidence = acceptance_record(args, "load")
    evidence["safety"].update(
        {
            "generated_bytes": args.generated_bytes,
            "quiescent_gate": True,
            "explicit_confirmation": True,
        }
    )
    failures: list[str] = []
    if evidence["source"].get("dirty"):
        failures.append("source_revision_dirty")
    if evidence["package"]["identity"] == "unavailable":
        failures.append("package_identity_unavailable")
    for service, state in evidence["services"].items():
        if state != "active":
            failures.append(f"{service}_not_active")
    before = staging_bytes()
    evidence["staging"].update(
        before_bytes=before, peak_bytes=before, after_bytes=before, recovered_bytes=0
    )
    source = pathlib.Path(args.validation_root) / "epic-c-load" / args.run_id
    source.mkdir(parents=True, mode=0o700)
    try:
        used = codex_used_bytes()
        if used + args.generated_bytes >= ONE_TIB:
            raise RuntimeError("CODEX existing plus generated bytes would reach 1 TiB")
        generate_random_workload(source, args.generated_bytes)
        ingest = subprocess.Popen(
            [
                "dasobjectstore",
                "ingest",
                "files",
                "CODEX",
                "--source",
                str(source),
                "--force",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        # Allow admission and source streaming to begin, then measure the two
        # HTTP surfaces while the data lane is occupied.
        time.sleep(args.load_warmup_seconds)
        if ingest.poll() is not None:
            failures.append("load_not_active_at_measurement")
        evidence["https"]["liveness"] = sample_url(
            f"{args.base_url}/api/v1/liveness",
            args.samples,
            args.cafile,
            args.cookie_file,
        )
        evidence["https"]["dashboard"] = sample_url(
            f"{args.base_url}/api/v1/dashboard/status",
            args.samples,
            args.cafile,
            args.cookie_file,
        )
        evidence["host"]["accept_queue_max"] = accept_queue(args.https_port)
        evidence["host"]["psi"] = read_psi()
        evidence["host"]["device_queue"] = device_queues()
        evidence["host"]["device_queue_max"] = max(
            (
                item["io_in_progress"]
                for item in evidence["host"]["device_queue"].values()
            ),
            default=0,
        )
        evidence["staging"]["peak_bytes"] = staging_bytes()

        started = time.monotonic()
        cancellation = subprocess.run(
            [
                "dasobjectstore",
                "ingest",
                "drain-queue",
                "CODEX",
                "--allow-ingest-queue-drain",
                "--confirm",
                "confirm ingest queue drain",
                "--reason",
                f"EPIC C CODEX acceptance {args.run_id}",
                "--json",
            ],
            check=False,
            capture_output=True,
            text=True,
            timeout=args.cancellation_timeout_seconds,
        )
        cancellation_ms = (time.monotonic() - started) * 1000
        evidence["cancellation"] = {
            "requested": True,
            "accepted": cancellation.returncode == 0,
            "latency_ms": round(cancellation_ms, 3),
        }
        try:
            ingest_output, _ = ingest.communicate(timeout=args.recovery_timeout_seconds)
        except subprocess.TimeoutExpired:
            # Do not kill or restart daemon-owned work. The CLI transport is
            # allowed to exit; durable ingest/cancellation state remains owned
            # by the daemon and the evidence fails closed.
            ingest.terminate()
            ingest_output, _ = ingest.communicate(timeout=10)
            failures.append("ingest_cli_did_not_settle")
        evidence["ingest"] = {
            "exit_code": ingest.returncode,
            "output_tail": ingest_output[-4096:],
        }
        deadline = time.monotonic() + args.recovery_timeout_seconds
        after = staging_bytes()
        while (
            after - before > evidence["thresholds"]["staging_recovery_bytes"]
            and time.monotonic() < deadline
        ):
            time.sleep(2)
            after = staging_bytes()
        evidence["staging"]["after_bytes"] = after
        evidence["staging"]["recovered_bytes"] = (
            evidence["staging"]["peak_bytes"] - after
        )
        mounts = evidence["telemetry"].get("mounted_devices", {})
        queues = evidence["host"].get("device_queue", {})
        mount_sources = json.dumps(mounts)
        evidence["telemetry"]["mapping_status"] = (
            "mapped"
            if queues
            and any(device in mount_sources for device in queues)
            else "unmapped"
        )
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        failures.append(str(error))
    finally:
        # Generated source is not managed ObjectStore data and is confined to
        # the approved validation root. ObjectStore payload is never removed
        # here; only daemon-owned lifecycle commands may reclaim it.
        shutil.rmtree(source, ignore_errors=True)
    thresholds = evidence["thresholds"]
    for surface in ("liveness", "dashboard"):
        measurement = evidence["https"][surface]
        if measurement["http_successes"] != measurement["samples"]:
            failures.append(f"{surface}_http_success")
        for quantile in ("p95", "p99"):
            if measurement[f"{quantile}_ms"] > thresholds[f"{surface}_{quantile}_ms"]:
                failures.append(f"{surface}_{quantile}_ms")
    if evidence["host"]["accept_queue_max"] > thresholds["accept_queue_max"]:
        failures.append("accept_queue_max")
    for resource in ("cpu", "io", "memory"):
        if psi_some_avg10(evidence["host"]["psi"], resource) > thresholds[
            f"{resource}_psi_some_avg10"
        ]:
            failures.append(f"{resource}_psi_some_avg10")
    if evidence["host"]["device_queue_max"] > thresholds["device_io_in_progress"]:
        failures.append("device_io_in_progress")
    if not evidence.get("cancellation", {}).get("accepted", False):
        failures.append("cancellation_accepted")
    elif evidence["cancellation"]["latency_ms"] > thresholds["cancellation_ms"]:
        failures.append("cancellation_ms")
    if evidence["telemetry"]["mapping_status"] != "mapped":
        failures.append("telemetry_mapping")
    if (
        evidence["staging"]["after_bytes"] - evidence["staging"]["before_bytes"]
        > thresholds["staging_recovery_bytes"]
    ):
        failures.append("staging_recovery")
    evidence["failures"] = sorted(set(failures))
    evidence["result"] = "failed" if failures else "passed"
    write_evidence(args.output, evidence)
    return validate(argparse.Namespace(evidence=args.output))


def validate(args: argparse.Namespace) -> int:
    evidence = json.loads(pathlib.Path(args.evidence).read_text())
    failures: list[str] = []
    if evidence.get("schema_version") != SCHEMA_VERSION:
        failures.append("schema_version")
    safety = evidence.get("safety", {})
    if safety.get("store_id") != "CODEX":
        failures.append("store_id")
    generated = safety.get("generated_bytes", -1)
    if not isinstance(generated, int) or generated < 0 or generated >= ONE_TIB:
        failures.append("generated_bytes")
    if safety.get("customer_or_project_data_used") is not False:
        failures.append("customer_or_project_data_used")
    if evidence.get("mode") == "load":
        if safety.get("quiescent_gate") is not True:
            failures.append("quiescent_gate")
        if safety.get("explicit_confirmation") is not True:
            failures.append("explicit_confirmation")
        thresholds = evidence.get("thresholds", {})
        for surface in ("liveness", "dashboard"):
            measurement = evidence.get("https", {}).get(surface) or {}
            for quantile in ("p95", "p99"):
                if measurement.get(f"{quantile}_ms", math.inf) > thresholds.get(
                    f"{surface}_{quantile}_ms", -1
                ):
                    failures.append(f"{surface}_{quantile}_ms")
        if evidence.get("host", {}).get("accept_queue_max", math.inf) > thresholds.get(
            "accept_queue_max", -1
        ):
            failures.append("accept_queue_max")
        host = evidence.get("host", {})
        for resource in ("cpu", "io", "memory"):
            if psi_some_avg10(host.get("psi", {}), resource) > thresholds.get(
                f"{resource}_psi_some_avg10", -1
            ):
                failures.append(f"{resource}_psi_some_avg10")
        if host.get("device_queue_max", math.inf) > thresholds.get(
            "device_io_in_progress", -1
        ):
            failures.append("device_io_in_progress")
        cancellation = evidence.get("cancellation") or {}
        if cancellation.get("accepted") is not True:
            failures.append("cancellation_accepted")
        if cancellation.get("latency_ms", math.inf) > thresholds.get("cancellation_ms", -1):
            failures.append("cancellation_ms")
        if evidence.get("telemetry", {}).get("mapping_status") != "mapped":
            failures.append("telemetry_mapping")
        staging = evidence.get("staging", {})
        if staging.get("after_bytes", math.inf) - staging.get("before_bytes", 0) > thresholds.get(
            "staging_recovery_bytes", -1
        ):
            failures.append("staging_recovery")
    if failures:
        print("EPIC C evidence failed: " + ", ".join(sorted(set(failures))), file=sys.stderr)
        return 1
    print("EPIC C evidence is structurally and safety valid")
    return 0


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser()
    sub = result.add_subparsers(dest="command", required=True)
    inspect_parser = sub.add_parser("inspect")
    inspect_parser.add_argument("--repo", required=True)
    inspect_parser.add_argument("--output", required=True)
    inspect_parser.add_argument("--base-url", default="https://127.0.0.1:8448/products/dasobjectstore")
    inspect_parser.add_argument("--cafile")
    inspect_parser.add_argument("--samples", type=int, default=20)
    inspect_parser.add_argument("--liveness-p95-ms", type=float, default=250)
    inspect_parser.add_argument("--liveness-p99-ms", type=float, default=500)
    inspect_parser.add_argument("--dashboard-p95-ms", type=float, default=1000)
    inspect_parser.add_argument("--dashboard-p99-ms", type=float, default=2000)
    inspect_parser.add_argument("--accept-queue-max", type=int, default=32)
    inspect_parser.add_argument("--cpu-psi-some-avg10", type=float, default=20)
    inspect_parser.add_argument("--io-psi-some-avg10", type=float, default=20)
    inspect_parser.add_argument("--memory-psi-some-avg10", type=float, default=5)
    inspect_parser.add_argument("--device-io-in-progress", type=int, default=64)
    inspect_parser.add_argument("--cancellation-ms", type=float, default=2000)
    inspect_parser.add_argument("--staging-recovery-bytes", type=int, default=67108864)
    load_parser = sub.add_parser("load")
    load_parser.add_argument("--repo", required=True)
    load_parser.add_argument("--output", required=True)
    load_parser.add_argument("--validation-root", required=True)
    load_parser.add_argument("--run-id", required=True)
    load_parser.add_argument("--cookie-file", required=True)
    load_parser.add_argument("--generated-bytes", required=True, type=int)
    load_parser.add_argument("--base-url", default="https://127.0.0.1:8448/products/dasobjectstore")
    load_parser.add_argument("--cafile")
    load_parser.add_argument("--samples", type=int, default=120)
    load_parser.add_argument("--https-port", type=int, default=8448)
    load_parser.add_argument("--load-warmup-seconds", type=int, default=2)
    load_parser.add_argument("--recovery-timeout-seconds", type=int, default=300)
    load_parser.add_argument("--cancellation-timeout-seconds", type=int, default=10)
    load_parser.add_argument("--liveness-p95-ms", type=float, default=250)
    load_parser.add_argument("--liveness-p99-ms", type=float, default=500)
    load_parser.add_argument("--dashboard-p95-ms", type=float, default=1000)
    load_parser.add_argument("--dashboard-p99-ms", type=float, default=2000)
    load_parser.add_argument("--accept-queue-max", type=int, default=32)
    load_parser.add_argument("--cpu-psi-some-avg10", type=float, default=20)
    load_parser.add_argument("--io-psi-some-avg10", type=float, default=20)
    load_parser.add_argument("--memory-psi-some-avg10", type=float, default=5)
    load_parser.add_argument("--device-io-in-progress", type=int, default=64)
    load_parser.add_argument("--cancellation-ms", type=float, default=2000)
    load_parser.add_argument("--staging-recovery-bytes", type=int, default=67108864)
    validate_parser = sub.add_parser("validate")
    validate_parser.add_argument("evidence")
    return result


if __name__ == "__main__":
    parsed = parser().parse_args()
    if parsed.command == "inspect":
        status = inspect(parsed)
    elif parsed.command == "load":
        status = load(parsed)
    else:
        status = validate(parsed)
    raise SystemExit(status)
