#!/usr/bin/env python3
"""Run a bounded LineageOS Wi-Fi on/off MSM-bus trace comparison.

Only runtime dynamic-debug flags and Wi-Fi enabled state are changed. The
script restores Wi-Fi, the original debug-print state, and unprivileged ADB in
its cleanup path. No boot image or partition is written.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import re
import subprocess
import sys
import threading
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CONTROL = "/proc/dynamic_debug/control"
UPDATES_RE = re.compile(r"update_request_adhoc|BW MBps: AB:|Err updating|Update path failed|error sending (?:active/awake|wake|sleep) sets|bandwidth request failed")
ANSI_RE = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
REGISTRATION_RE = re.compile(
    r"register_client_adhoc:(?:find path\.src|Failed to find path\.src|Client handle)"
)
IPA_ROUTE_ENDPOINTS = ((90, 512), (90, 585), (1, 676), (143, 777))
IPA_USECASES = {
    0: ("MIN", None),
    1: ("SVS2", 50_000_000),
    2: ("SVS", 100_000_000),
    3: ("NOMINAL", 201_000_000),
    4: ("TURBO", 240_000_000),
}
ADB_SERIAL: str | None = None
CONNECTED_RE = re.compile(
    r"(?i)(?:state[:= ]+CONNECTED/CONNECTED|supplicant state[:= ]*COMPLETED|supplicantstate[:= ]*COMPLETED)"
)


def strip_ansi(line: str) -> str:
    return ANSI_RE.sub("", line)


def registration_lines(dmesg: str) -> str:
    """Keep only bus registration records; drop unrelated kernel log content."""
    lines = [strip_ansi(line) for line in dmesg.splitlines() if REGISTRATION_RE.search(line)]
    if not lines:
        raise RuntimeError("current-boot dmesg contains no MSM-bus registration records")
    return "\n".join(lines) + "\n"


def validate_runtime_registration_snapshot(inventory: dict) -> dict:
    """Require a complete, uniquely registered four-route IPA client."""
    snapshot = inventory.get("runtime_registration_snapshot", {})
    registrations = snapshot.get("registrations", [])
    handles = [row.get("handle") for row in registrations]
    if not registrations or len(set(handles)) != len(handles):
        raise RuntimeError("same-boot MSM-bus registration snapshot is empty or has duplicate handles")
    ipa = [row for row in registrations if row.get("name") == "ipa"]
    if len(ipa) != 1:
        raise RuntimeError(f"expected one same-boot IPA bus client, found {len(ipa)}")
    paths = ipa[0].get("paths", [])
    endpoints = [(path.get("source_id"), path.get("destination_id")) for path in paths]
    resolved = all(isinstance(path.get("route"), list) and path["route"] for path in paths)
    if len(endpoints) != len(IPA_ROUTE_ENDPOINTS) or set(endpoints) != set(IPA_ROUTE_ENDPOINTS) or not resolved:
        raise RuntimeError(f"same-boot IPA route set is incomplete or changed: {endpoints!r}")
    return snapshot.get("summary", {})


def reuse_same_boot_inventory(
    current_fdt: Path, current_uptime: float
) -> tuple[Path, dict] | None:
    """Reuse a saved registration capture only when it is provably same-boot."""
    current_hash = hashlib.sha256(current_fdt.read_bytes()).hexdigest()
    current_lineage = adb("shell", "getprop", "ro.build.display.id")
    current_kernel = adb("shell", "uname", "-r")
    now = time.time()
    roots = sorted(
        (ROOT / ".cache/pixel-sunfish/msm-bus-update-trace").glob("wifi-compare-*/"),
        key=lambda path: path.stat().st_mtime,
        reverse=True,
    )
    for folder in roots:
        marker_path = folder / "wifi-window-markers.json"
        inventory_path = folder / "msm-bus-runtime-inventory.json"
        if not marker_path.is_file() or not inventory_path.is_file():
            continue
        try:
            markers = json.loads(marker_path.read_text())
            inventory = json.loads(inventory_path.read_text())
            events = markers.get("events", [])
            restored = next(
                event["uptime_seconds"]
                for event in reversed(events)
                if event.get("event") == "restore_verified"
            )
            elapsed_device = current_uptime - float(restored)
            elapsed_host = now - marker_path.stat().st_mtime
            same_boot = (
                0 <= elapsed_device
                and abs(elapsed_device - elapsed_host) <= 60
                and markers.get("lineage") == current_lineage
                and markers.get("kernel_release") == current_kernel
                and inventory.get("dtb_sha256") == current_hash
            )
            if not same_boot:
                continue
            summary = validate_runtime_registration_snapshot(inventory)
            ipa_handle = next(
                row["handle"]
                for row in inventory["runtime_registration_snapshot"]["registrations"]
                if row.get("name") == "ipa"
            )
            return inventory_path, {
                "runtime_fdt_sha256": current_hash,
                "registration_inventory_path": str(inventory_path.resolve()),
                "registration_lines_sha256": inventory[
                    "runtime_registration_snapshot"
                ]["dmesg_sha256"],
                "registration_summary": summary,
                "ipa_handle": ipa_handle,
                "scope": (
                    "reused prior registration capture after exact runtime-FDT, "
                    "Lineage, kernel, and continuous-uptime same-boot checks"
                ),
            }
        except (KeyError, StopIteration, OSError, TypeError, ValueError, RuntimeError):
            continue
    return None


def capture_runtime_inventory(out_dir: Path) -> tuple[Path, dict]:
    """Capture this boot's FDT and bus registrations without storing full dmesg."""
    fdt_path = out_dir / "lineage-runtime.dtb"
    registration_log_path = out_dir / "msm-bus-registration-lines.txt"
    inventory_path = out_dir / "msm-bus-runtime-inventory.json"

    fdt_command = [
        "python3",
        str(ROOT / "platforms/sunfish/capture-live-fdt.py"),
    ]
    if ADB_SERIAL:
        fdt_command += ["--serial", ADB_SERIAL]
    fdt_command += ["--output", str(fdt_path)]
    run(fdt_command, timeout=60)
    try:
        filtered_registrations = registration_lines(adb("exec-out", "dmesg", timeout=30))
    except RuntimeError:
        reused = reuse_same_boot_inventory(fdt_path, uptime())
        if reused is None:
            raise
        print(
            "current dmesg registration lines expired; reusing a verified same-boot inventory",
            flush=True,
        )
        return reused
    registration_log_path.write_text(filtered_registrations)
    run([
        "python3",
        str(ROOT / "platforms/sunfish/audit-msm-bus-client-inventory.py"),
        str(fdt_path),
        "--dmesg",
        str(registration_log_path),
        "--output",
        str(inventory_path),
    ], timeout=120)
    inventory = json.loads(inventory_path.read_text())
    summary = validate_runtime_registration_snapshot(inventory)
    return inventory_path, {
        "runtime_fdt_sha256": inventory["dtb_sha256"],
        "registration_inventory_path": str(inventory_path.resolve()),
        "registration_lines_sha256": inventory["runtime_registration_snapshot"]["dmesg_sha256"],
        "registration_summary": summary,
        "ipa_handle": next(
            row["handle"]
            for row in inventory["runtime_registration_snapshot"]["registrations"]
            if row["name"] == "ipa"
        ),
        "scope": "same-boot FDT and filtered MSM-bus registration log",
    }


def run(command: list[str], *, timeout: int = 30, check: bool = True) -> str:
    result = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=timeout)
    if check and result.returncode != 0:
        raise RuntimeError(f"command failed ({result.returncode}): {' '.join(command)}\n{result.stdout[-2000:]}")
    return result.stdout.strip()


def adb(*args: str, timeout: int = 30, check: bool = True) -> str:
    command = ["adb"]
    if ADB_SERIAL:
        command += ["-s", ADB_SERIAL]
    return run(command + list(args), timeout=timeout, check=check)


def authorized_adb_serials(output: str) -> list[str]:
    return [
        fields[0]
        for line in output.splitlines()[1:]
        if len(fields := line.split()) >= 2 and fields[1] == "device"
    ]


def device_count() -> int:
    serials = authorized_adb_serials(run(["adb", "devices"], timeout=15))
    if ADB_SERIAL:
        return int(ADB_SERIAL in serials)
    return len(serials)


def wifi_status() -> str:
    enabled = adb("shell", "settings get global wifi_on").strip()
    text = adb("shell", "dumpsys wifi", timeout=30)
    if enabled == "0":
        return "disabled"
    return "connected" if CONNECTED_RE.search(text) else "enabled-disconnected"


def uptime() -> float:
    return float(adb("shell", "cat /proc/uptime").split()[0])


def control_records() -> list[str]:
    output = adb("shell", f"grep -E 'update_request_adhoc|set_bw' {CONTROL}")
    records = [
        line for line in output.splitlines()
        if "update_request_adhoc" in line
        or ("drivers/devfreq/devfreq_devbw.c:" in line and "]set_bw " in line)
    ]
    if not any("update_request_adhoc" in line for line in records) or not any("set_bw" in line for line in records):
        raise RuntimeError("expected dynamic-debug callsites were not found")
    return records


def restore_dynamic_debug() -> list[str]:
    """Disable both trace callsites independently, then verify their state."""
    command_errors: list[str] = []
    for label, command in (
        ("update_request_adhoc", f"echo 'func update_request_adhoc -p' > {CONTROL}"),
        ("set_bw", f"echo 'func set_bw -p' > {CONTROL}"),
    ):
        try:
            adb("shell", command)
        except BaseException as exc:
            # A failed command must not prevent the other callsite from being
            # restored. The final readback decides whether cleanup succeeded.
            command_errors.append(f"{label}: {type(exc).__name__}: {exc}")

    try:
        restored = control_records()
    except BaseException as exc:
        details = "; ".join(command_errors)
        suffix = f"; disable attempts: {details}" if details else ""
        raise RuntimeError(f"could not verify dynamic-debug cleanup{suffix}") from exc

    enabled = [line for line in restored if "p" in flags(line)]
    if enabled:
        details = "; ".join(command_errors)
        suffix = f"; disable attempts: {details}" if details else ""
        raise RuntimeError(
            "dynamic-debug print flags remain enabled after cleanup: "
            + "; ".join(enabled)
            + suffix
        )
    return restored


def flags(line: str) -> str:
    match = re.search(r"=([a-zA-Z_]+)\s+\"", line)
    if not match:
        raise RuntimeError(f"cannot parse dynamic-debug flags: {line}")
    return match.group(1)


def connected_state() -> bool:
    return wifi_status() == "connected"


def wait_wifi(expected: str, timeout: int) -> str:
    deadline = time.monotonic() + timeout
    last = "unknown"
    while time.monotonic() < deadline:
        last = wifi_status()
        if last == expected:
            return last
        time.sleep(1)
    raise TimeoutError(f"Wi-Fi did not reach {expected}; last state was {last}")


class TraceStream:
    def __init__(self, path: Path):
        self.path = path
        command = ["adb"]
        if ADB_SERIAL:
            command += ["-s", ADB_SERIAL]
        self.process = subprocess.Popen(
            command + ["exec-out", "dmesg", "-W"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            errors="replace",
            bufsize=1,
        )
        self.thread = threading.Thread(target=self._copy, daemon=True)
        self.thread.start()
        time.sleep(0.5)
        if self.process.poll() is not None:
            detail = self.process.stderr.read().strip() if self.process.stderr else ""
            raise RuntimeError(f"live dmesg stream exited before the comparison: {detail}")

    def _copy(self) -> None:
        assert self.process.stdout is not None
        with self.path.open("w", encoding="utf-8") as output:
            for line in self.process.stdout:
                clean = strip_ansi(line)
                if UPDATES_RE.search(clean):
                    output.write(clean)
                    output.flush()

    def stop(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        self.thread.join(timeout=5)


def comparison_report(trace_path: Path, marker_path: Path, inventory_path: Path, output_path: Path) -> dict:
    audit_path = ROOT / "platforms/sunfish/audit-msm-bus-request-trace.py"
    spec = importlib.util.spec_from_file_location("msm_bus_audit", audit_path)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load the MSM-bus trace parser")
    audit = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(audit)

    trace_bytes = trace_path.read_bytes()
    parsed = audit.parse_trace(trace_bytes.decode("utf-8", errors="replace"))
    markers = json.loads(marker_path.read_text())
    inventory = json.loads(inventory_path.read_text())
    snapshot = inventory.get("runtime_registration_snapshot", {})
    registrations = snapshot.get("registrations", [])
    by_handle = {int(row["handle"]): row for row in registrations}
    if len(by_handle) != len(registrations) or not by_handle:
        raise RuntimeError("runtime registration snapshot is missing or has duplicate handles")

    updates = parsed["request_updates"]
    unknown_handles = sorted({row["handle"] for row in updates} - set(by_handle))
    path_count_mismatches = 0
    path_mismatch_handles: dict[int, dict] = {}
    prior_index_mismatches = 0
    last_index: dict[int, int] = {}
    for row in updates:
        registration = by_handle.get(row["handle"])
        if registration is None:
            continue
        expected_paths = len(registration.get("paths", []))
        if expected_paths and row["num_paths"] != expected_paths:
            path_count_mismatches += 1
            row["client_name"] = f"snapshot-mismatch:{registration['name']}"
            mismatch = path_mismatch_handles.setdefault(row["handle"], {
                "snapshot_name": registration["name"],
                "snapshot_path_count": expected_paths,
                "observed_path_counts": {},
                "updates": 0,
            })
            mismatch["observed_path_counts"][str(row["num_paths"])] = \
                mismatch["observed_path_counts"].get(str(row["num_paths"]), 0) + 1
            mismatch["updates"] += 1
        else:
            row["client_name"] = registration["name"]
        previous = last_index.get(row["handle"])
        if previous is not None and row["reported_prior_index"] != previous:
            prior_index_mismatches += 1
        last_index[row["handle"]] = row["index"]

    event_times = {row["event"]: float(row["uptime_seconds"]) for row in markers["events"]}
    phases = dict(markers["phases"])
    phases["wifi_enable_to_connected"] = {
        "start_seconds": event_times["wifi_enable_requested"],
        "end_seconds": event_times["wifi_on_connected"],
    }
    windows = []
    for name, phase in phases.items():
        start, end = float(phase["start_seconds"]), float(phase["end_seconds"])
        phase_updates = [row for row in updates if row.get("kernel_time_seconds") is not None
                         and start <= row["kernel_time_seconds"] < end]
        by_client: dict[str, dict] = {}
        for row in phase_updates:
            client = f"{row.get('client_name', 'unknown')}#{row['handle']}"
            summary = by_client.setdefault(client, {"name": row.get("client_name", "unknown"),
                "handle": row["handle"], "updates": 0, "transitions": {},
                "usecase_transitions": {}, "events": []})
            transition = f"{row['reported_prior_index']}->{row['index']}"
            summary["updates"] += 1
            summary["transitions"][transition] = summary["transitions"].get(transition, 0) + 1
            if summary["name"] == "ipa":
                before = IPA_USECASES.get(row["reported_prior_index"])
                after = IPA_USECASES.get(row["index"])
                event = {"uptime_seconds": row["kernel_time_seconds"], "transition": transition}
                if before is not None and after is not None:
                    named_transition = f"{before[0]}->{after[0]}"
                    summary["usecase_transitions"][named_transition] = (
                        summary["usecase_transitions"].get(named_transition, 0) + 1
                    )
                    event.update({
                        "from_usecase": before[0],
                        "to_usecase": after[0],
                        "from_ipa_rate_selector_hz": before[1],
                        "to_ipa_rate_selector_hz": after[1],
                    })
                summary["events"].append(event)
        phase_devbw = [row for row in parsed["devbw_samples"] if row.get("kernel_time_seconds") is not None
                       and start <= row["kernel_time_seconds"] < end]
        devbw_by_device: dict[str, int] = {}
        for row in phase_devbw:
            device = row["device_name"]
            devbw_by_device[device] = devbw_by_device.get(device, 0) + 1
        windows.append({
            "name": name,
            "start_seconds": start,
            "end_seconds": end,
            "duration_seconds": round(end - start, 2),
            "request_update_count": len(phase_updates),
            "devbw_sample_count": len(phase_devbw),
            "clients_with_updates": len(by_client),
            "updates_by_client": by_client,
            "devbw_by_device": devbw_by_device,
        })

    report = {
        "device": "sunfish",
        "lineage": markers.get("lineage"),
        "trace_sha256": hashlib.sha256(trace_bytes).hexdigest(),
        "trace_bytes": len(trace_bytes),
        "scope": "Runtime-only MSM-bus request and devfreq trace across connected/off/reconnected Wi-Fi windows; no WLAN-specific vote attribution.",
        "registration_association": {
            "source": str(inventory_path),
            "registered_handles": len(by_handle),
            "scope": snapshot.get("scope"),
            "same_boot_capture": markers.get("registration_capture"),
            "note": "Names and route counts are joined from the comparison boot's registration snapshot; path-count mismatches remain unresolved.",
        },
        "summary": {
            "request_updates": len(updates),
            "updates_with_known_snapshot_handle": len(updates) - len(unknown_handles),
            "unknown_handles": unknown_handles,
            "devbw_samples": len(parsed["devbw_samples"]),
            "request_lines_without_timestamp": sum("kernel_time_seconds" not in row for row in updates),
            "path_count_mismatches": path_count_mismatches,
            "path_count_mismatch_handles": path_mismatch_handles,
            "updates_with_path_count_match": len(updates) - path_count_mismatches - len(unknown_handles),
            "prior_index_mismatches_after_first_sample": prior_index_mismatches,
            "bus_error_lines": len(parsed["bus_error_lines"]),
            "failed_paths": len(parsed["failed_paths"]),
            "unchanged_index_notes_without_handle": len(parsed["unchanged_index_notes_without_handle"]),
        },
        "windows": windows,
        "capture_stream": "Filtered live dmesg stream; ANSI color codes removed before parsing.",
        "ssid_and_network_identifiers_saved": False,
    }
    output_path.write_text(json.dumps(report, indent=2) + "\n")
    return report


def main() -> int:
    global ADB_SERIAL
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--window-seconds", type=int, default=30)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--serial", help="ADB serial when other devices such as emulators are attached")
    args = parser.parse_args()
    ADB_SERIAL = args.serial
    if args.window_seconds < 5 or args.window_seconds > 120:
        parser.error("window length must be between 5 and 120 seconds")
    if device_count() != 1:
        parser.error("expected one authorized ADB device; pass --serial when multiple devices are attached")
    if adb("shell", "getprop ro.product.device") != "sunfish":
        parser.error("selected ADB device is not Sunfish")
    if wifi_status() != "connected":
        parser.error("Wi-Fi must be enabled and connected before the comparison")

    out_dir = args.output_dir or (
        ROOT / ".cache/pixel-sunfish/msm-bus-update-trace"
        / f"wifi-compare-repeat-{dt.datetime.now(dt.timezone.utc).strftime('%Y%m%d-%H%M%S')}"
    )
    out_dir.mkdir(parents=True, exist_ok=False)
    marker_path = out_dir / "wifi-window-markers.json"
    trace_path = out_dir / "msm-bus-update-trace.txt"
    events: list[dict] = []
    stream: TraceStream | None = None
    rooted = False
    flags_enabled = False
    initial_wifi = "connected"
    original_records: list[str] = []
    failure: BaseException | None = None

    def mark(event: str, expected: str) -> float:
        state = wifi_status()
        if state != expected:
            raise RuntimeError(f"{event}: expected Wi-Fi {expected}, observed {state}")
        seconds = uptime()
        events.append({"event": event, "uptime_seconds": round(seconds, 2), "wifi_state": state})
        print(f"{event}: uptime={seconds:.2f}s wifi={state}", flush=True)
        return seconds

    try:
        root_output = adb("root", timeout=15)
        rooted = True
        print(root_output, flush=True)
        adb("wait-for-device", timeout=60)
        if "uid=0(root)" not in adb("shell", "id"):
            raise RuntimeError("temporary root ADB was not available")

        inventory_path, registration_capture = capture_runtime_inventory(out_dir)

        original_records = control_records()
        if any("p" in flags(line) for line in original_records):
            raise RuntimeError("a target dynamic-debug callsite was already enabled; refusing to alter prior state")
        (out_dir / "dynamic-debug-before.txt").write_text("\n".join(original_records) + "\n")

        flags_enabled = True
        adb("shell", f"echo 'func update_request_adhoc +p' > {CONTROL}")
        adb("shell", f"echo 'func set_bw +p' > {CONTROL}")
        active_records = control_records()
        if any("p" not in flags(line) for line in active_records):
            raise RuntimeError("not all target dynamic-debug callsites enabled")
        (out_dir / "dynamic-debug-during.txt").write_text("\n".join(active_records) + "\n")

        stream = TraceStream(trace_path)
        window = args.window_seconds
        baseline_start = mark("baseline_start", "connected")
        time.sleep(window)
        baseline_end = mark("baseline_end", "connected")

        mark("wifi_disable_requested", "connected")
        adb("shell", "cmd wifi set-wifi-enabled disabled", timeout=20)
        wait_wifi("disabled", 20)
        off_start = mark("wifi_off_confirmed", "disabled")
        time.sleep(window)
        off_end = mark("wifi_off_end", "disabled")

        mark("wifi_enable_requested", "disabled")
        adb("shell", "cmd wifi set-wifi-enabled enabled", timeout=20)
        wait_wifi("connected", 120)
        on_start = mark("wifi_on_connected", "connected")
        time.sleep(window)
        on_end = mark("wifi_on_end", "connected")
        mark("restore_verified", "connected")
        stream.stop()
        stream = None

        marker = {
            "device": "sunfish",
            "lineage": adb("shell", "getprop ro.build.display.id"),
            "kernel_release": adb("shell", "uname", "-r"),
            "registration_capture": registration_capture,
            "window_seconds": window,
            "capture": "runtime dynamic-debug; filtered live dmesg stream",
            "events": events,
            "phases": {
                "baseline": {"start_seconds": baseline_start, "end_seconds": baseline_end},
                "wifi_off": {"start_seconds": off_start, "end_seconds": off_end},
                "wifi_on_connected": {"start_seconds": on_start, "end_seconds": on_end},
            },
            "restored_state": "connected",
            "trace_path": trace_path.name,
            "ssid_and_network_identifiers_saved": False,
        }
        marker_path.write_text(json.dumps(marker, indent=2) + "\n")
    except BaseException as exc:
        failure = exc
    finally:
        if stream is not None:
            stream.stop()
        if rooted:
            if flags_enabled:
                try:
                    restored = restore_dynamic_debug()
                    (out_dir / "dynamic-debug-after.txt").write_text("\n".join(restored) + "\n")
                    flags_enabled = False
                except BaseException as exc:
                    failure = failure or exc
            try:
                if initial_wifi == "connected" and not connected_state():
                    adb("shell", "cmd wifi set-wifi-enabled enabled", timeout=20)
                    wait_wifi("connected", 120)
                if wifi_status() != initial_wifi:
                    raise RuntimeError("Wi-Fi state was not restored")
            except BaseException as exc:
                failure = failure or exc
            try:
                adb("unroot", timeout=15)
                adb("wait-for-device", timeout=60)
                if "uid=0(root)" in adb("shell", "id"):
                    raise RuntimeError("ADB remained root after adb unroot")
            except BaseException as exc:
                failure = failure or exc

    if failure is not None:
        partial = {
            "device": "sunfish",
            "events": events,
            "restoration_attempted": True,
            "error": f"{type(failure).__name__}: {failure}",
            "ssid_and_network_identifiers_saved": False,
        }
        marker_path.write_text(json.dumps(partial, indent=2) + "\n")
        print(f"comparison stopped; cleanup attempted; partial markers: {marker_path}", file=sys.stderr)
        print(str(failure), file=sys.stderr)
        return 1

    report_path = out_dir / "msm-bus-update-trace-windows.json"
    report = comparison_report(trace_path, marker_path, inventory_path, report_path)
    print(json.dumps(report["summary"], indent=2), flush=True)
    print(f"markers={marker_path}\ntrace={trace_path}\nreport={report_path}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
