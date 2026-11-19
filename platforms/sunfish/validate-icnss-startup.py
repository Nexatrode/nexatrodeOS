#!/usr/bin/env python3
"""Audit Sunfish ICNSS cold-server setup versus PDR recovery in pinned Linux."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PINNED_COMMIT = "28f9290ae067d3e7857126eb6312720dbe9c7c80"
DEFAULT_SOURCE = ROOT / ".cache/sunfish-lineage-kernel"
AUDITED_PATHS = (
    "drivers/soc/qcom/icnss.c",
    "drivers/soc/qcom/service-notifier.c",
)


def git(source: Path, *args: str) -> str:
    return subprocess.check_output(["git", "-C", str(source), *args], text=True)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def c_function(text: str, signature: str) -> str:
    """Return one C function body, ignoring braces inside comments/literals."""
    start = text.index(signature)
    brace = text.index("{", start)
    depth = 0
    i = brace
    state = "code"
    while i < len(text):
        ch = text[i]
        nxt = text[i + 1] if i + 1 < len(text) else ""
        if state == "code":
            if ch == "/" and nxt == "*":
                state = "block_comment"
                i += 2
                continue
            if ch == "/" and nxt == "/":
                state = "line_comment"
                i += 2
                continue
            if ch == '"':
                state = "string"
            elif ch == "'":
                state = "char"
            elif ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    return text[brace : i + 1]
        elif state == "block_comment" and ch == "*" and nxt == "/":
            state = "code"
            i += 2
            continue
        elif state == "line_comment" and ch == "\n":
            state = "code"
        elif state in ("string", "char"):
            if ch == "\\":
                i += 2
                continue
            if (state == "string" and ch == '"') or (state == "char" and ch == "'"):
                state = "code"
        i += 1
    raise SystemExit(f"FAIL: unterminated function body for {signature}")


def ordered(body: str, tokens: tuple[str, ...], label: str) -> None:
    positions = [body.find(token) for token in tokens]
    require(all(position >= 0 for position in positions), f"{label}: missing {tokens}")
    require(positions == sorted(positions), f"{label}: order changed for {tokens}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    args = parser.parse_args()
    source = args.source.resolve()
    require(source.is_dir(), f"source tree not found: {source}")
    commit = git(source, "rev-parse", "HEAD").strip()
    require(commit == PINNED_COMMIT, f"expected {PINNED_COMMIT}, got {commit}")
    for path in AUDITED_PATHS:
        clean = subprocess.run(
            ["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", path],
            check=False,
        )
        require(clean.returncode == 0, f"audited source file has local edits: {path}")

    icnss = git(source, "show", f"HEAD:{AUDITED_PATHS[0]}")
    notifier = git(source, "show", f"HEAD:{AUDITED_PATHS[1]}")
    arrive = c_function(icnss, "static int icnss_driver_event_server_arrive(void *data)")
    locator_notify = c_function(
        icnss,
        "static int icnss_get_service_location_notify(struct notifier_block *nb,",
    )
    pd_enable = c_function(icnss, "static int icnss_pd_restart_enable(struct icnss_priv *priv)")
    recovery_enable = c_function(icnss, "static int icnss_enable_recovery(struct icnss_priv *priv)")
    trigger = c_function(icnss, "int icnss_trigger_recovery(struct device *dev)")
    new_server = c_function(notifier, "static void new_server_work(struct work_struct *work)")

    ordered(arrive, (
        "icnss_connect_to_fw_server(penv, data)",
        "icnss_hw_power_on(penv)",
        "wlfw_ind_register_send_sync_msg(penv)",
        "wlfw_msa_mem_info_send_sync_msg(penv)",
        "icnss_assign_msa_perm_all(penv,",
        "wlfw_msa_ready_send_sync_msg(penv)",
        "wlfw_cap_send_sync_msg(penv)",
        "wlfw_dynamic_feature_mask_send_sync_msg(penv,",
    ), "WLFW server-arrival setup")
    require("icnss_trigger_recovery" not in arrive
            and "service_notif_pd_restart" not in arrive,
            "WLFW server-arrival setup now invokes PDR/recovery")

    require("get_service_location(" in pd_enable,
            "PDR enable path no longer resolves the service location")
    require("service_notif_pd_restart" not in pd_enable,
            "PDR enable path now initiates a restart instead of registering/resolving")
    require("opcode != LOCATOR_UP" in locator_notify
            and "pd->total_domains == 0" in locator_notify
            and "service_notif_register_notifier(" in locator_notify
            and "ICNSS_PDR_REGISTERED" in locator_notify,
            "service-location callback no longer validates domains and registers PDR notifiers")
    require("icnss_pd_restart_enable(priv)" in recovery_enable,
            "ICNSS recovery setup no longer enables the PDR listener")
    require("icnss_trigger_recovery" not in recovery_enable
            and "service_notif_pd_restart" not in recovery_enable,
            "recovery enable path now initiates PDR during registration")

    for guard in ("ICNSS_PD_RESTART", "ICNSS_PDR_REGISTERED", "service_notifier"):
        require(guard in trigger, f"PDR trigger lost its {guard} guard")
    require("service_notif_pd_restart(" in trigger,
            "explicit PDR recovery trigger no longer sends the restart request")
    ordered(new_server, (
        "register_notif_listener(service_notif, data,",
        "service_notif_queue_notification(",
    ), "Service Notifier new-server handling")

    print(f"PASS: pinned Lineage Sunfish kernel {commit}; audited source files are clean")
    print("PASS: WLFW server arrival connects, powers the WLAN path, and performs the ICNSS QMI/MSA sequence")
    print("PASS: initial WLFW server-arrival callback does not invoke PDR")
    print("PASS: PDR setup resolves domains and registers state notifiers; explicit restart is guarded recovery")
    print("PASS: Service Notifier registers a listener before delivering current state")
    print("LIMIT: source audit does not identify the private initial wlan_pd launch trigger")


if __name__ == "__main__":
    main()
