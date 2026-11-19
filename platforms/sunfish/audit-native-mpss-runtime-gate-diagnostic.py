#!/usr/bin/env python3
"""Audit the diagnostic-only MPSS gate path and its visible checkpoints."""
from pathlib import Path
import argparse
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[2]
PORT = ROOT / "platforms/sunfish"
DIAGNOSTIC = "sunfish-native-startup-mpss-runtime-gate-diagnostic"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def compact(text: str) -> str:
    return re.sub(r"\s+", "", text)


def function_body(source: str, signature: str) -> str:
    start = source.index(signature)
    opening = source.index("{", start)
    depth = 0
    for i in range(opening, len(source)):
        if source[i] == "{":
            depth += 1
        elif source[i] == "}":
            depth -= 1
            if depth == 0:
                return source[opening + 1:i]
    raise ValueError("unterminated function body: " + signature)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()

    cargo = (ROOT / "rust/Cargo.toml").read_text()
    auth_path = ROOT / "rust/src/pixel_mpss_auth.rs"
    auth = auth_path.read_text()
    rmtfs_path = ROOT / "rust/src/pixel_linux_rmtfs.rs"
    rmtfs = rmtfs_path.read_text()
    qcom_path = ROOT / "rust/src/qcom_glink_qmi.rs"
    qcom = qcom_path.read_text()
    startup_path = ROOT / "rust/src/sunfish_native_startup_preflight.rs"
    startup = startup_path.read_text()
    receive_path = ROOT / "rust/src/pixel_ipa_receive.rs"
    receive = receive_path.read_text()
    smmu_path = ROOT / "rust/src/pixel_smmu_install.rs"
    smmu = smmu_path.read_text()
    native_smmu_path = ROOT / "rust/src/sunfish_smmu_native.rs"
    native_smmu = native_smmu_path.read_text()
    irq_painter_paths = {
        name: ROOT / f"rust/src/{name}.rs"
        for name in (
            "pixel_ipa_gsi", "pixel_ipa_command", "pixel_ipa_transfer",
            "pixel_ipa_tables",
        )
    }
    irq_painters = {name: path.read_text() for name, path in irq_painter_paths.items()}
    builder_path = PORT / "build-native-mpss-runtime-gate-diagnostic.py"
    builder = builder_path.read_text()
    preflight_builder = (PORT / "build-native-startup-preflight.sh").read_text()
    require('"--native-source-report-guard"' in builder
            and '"--native-startup-entry"' not in builder,
            "diagnostic builder selects labeled source failures and pass-through")
    require('elif [ "$runtime_mode" = true ]; then' in preflight_builder
            and '--native-source-report-guard' in preflight_builder,
            "runtime modes select the labeled pass-through source guard")

    require(
        f'{DIAGNOSTIC} = ["sunfish-native-startup-mpss-runtime"]' in cargo,
        "diagnostic feature is layered on the runtime feature",
    )

    start = function_body(auth, "pub(super) fn start(d: &Display, s: &mut Startup)")
    start_compact = compact(start)
    mark = "ENTRY_ATTEMPTED.store(1,Ordering::Release);"
    normal = "start_runtime(d,s);"
    require(start_compact.index(mark) < start_compact.index(normal),
            "auth-attempt marker is stored before the shared runtime entry")
    runtime_entry = function_body(auth, "fn start_runtime(d: &Display, s: &mut Startup)")
    runtime_entry_compact = compact(runtime_entry)
    require(
        'show_gate_diagnostic(' in runtime_entry_compact
        and 'cfg!(feature="sunfish-native-startup-mpss-runtime-gate-diagnostic")' in runtime_entry_compact,
        "runtime and diagnostic use the same gate call path with only the stop policy varied",
    )
    require(
        f'#[cfg(feature="{DIAGNOSTIC}")]return;' in runtime_entry_compact
        and 'start_runtime_after_gate(d,s);' in runtime_entry_compact,
        "diagnostic returns before the separately cfg-gated PAS4 handoff helper",
    )

    require(
        re.search(
            r'#\[cfg\(feature\s*=\s*"sunfish-native-startup-mpss-runtime"\)\]\s*'
            r'static ENTRY_ATTEMPTED:',
            auth,
        ) is not None,
        "attempt marker exists only in the diagnostic feature",
    )
    require(
        re.search(
            r'#\[cfg\(feature\s*=\s*"sunfish-native-startup-mpss-runtime"\)\]\s*'
            r'pub\(super\) fn diagnostics_owned',
            auth,
        ) is not None,
        "display ownership API exists only in the diagnostic feature",
    )

    gate = function_body(
        auth,
        "fn show_gate_diagnostic(d: &Display, s: &Startup, stop_after_report: bool)",
    )
    checkpoints = [
        (r'replace_line\(2,\s*"CHECK PREAUTH VERIFIED"\);\s*d\.flush\(\);\s*let preauth_verified\s*=',
         "preauth check has a flushed checkpoint"),
        (r'replace_line\(3,\s*"CHECK IPA RX HEALTH"\);\s*d\.flush\(\);\s*let ipa_rx_healthy\s*=',
         "IPA health check has a flushed checkpoint"),
        (r'replace_line\(4,\s*"CHECK DOMAIN QUIET"\);\s*d\.flush\(\);\s*let domain_quiet\s*=',
         "domain check has a flushed checkpoint"),
        (r'replace_line\(5,\s*"CHECK RMTFS GENERATION"\);\s*d\.flush\(\);\s*let retained\s*=',
         "RMTFS generation check has a flushed checkpoint"),
        (r'replace_line\(7,\s*"FACTORY SNAPSHOT START"\);\s*d\.flush\(\);\s*let factory_report\s*=',
         "factory snapshot has a flushed checkpoint"),
        (r'replace_line\(13,\s*"PROFILE READ START"\);\s*d\.flush\(\);\s*let profile\s*=\s*factory\.runtime_profile_report_with\(',
         "runtime profile read follows a flushed start line"),
        (r'replace_line\(23,\s*"CHECK PREBOOT GENERATION"\);\s*d\.flush\(\);\s*let preboot\s*=',
         "preboot-generation check has a flushed checkpoint"),
    ]
    for needle, reason in checkpoints:
        require(re.search(needle, gate) is not None, reason)

    require('Stage::ContextControl=>("DOMCBCONTROL","REGADDRESS",0x1508_4000)' in compact(gate)
            and 'Stage::ContextControlValue=>("DOMCBCTRLOK","CBSCTLRVALUE",indexasu32)' in compact(gate)
            and 'Stage::ContextFaultStatus=>("DOMFSRCONTEXT","REGADDRESS",0x1508_4058)' in compact(gate),
            "context-bank control is reported before context FSR with both physical addresses")
    for stage, label in (
        ("FaultStatusRejected", "FSRREJECTED"),
        ("FaultStatusAccepted", "FSRACCEPTED"),
        ("ReceiveRecheckStart", "RECHECKSTART"),
    ):
        require(f"Stage::{stage}=>({label!r}".replace("'", '"') in compact(gate),
                "display exposes the " + stage + " checkpoint")
    retained_gate = function_body(
        smmu,
        "pub(crate) fn retained_quiet_with("
    )
    retained_gate_compact = compact(retained_gate)
    accepted = "progress(Stage::FaultStatusAccepted,contextasusize);"
    rejected = "progress(Stage::FaultStatusRejected,contextasusize);"
    recheck = "progress(Stage::ReceiveRecheckStart,0);"
    call = "adapter.retained_receive_domain_with(&mutprogress).is_ok()"
    require(rejected in retained_gate_compact
            and accepted in retained_gate_compact
            and recheck in retained_gate_compact
            and retained_gate_compact.index(rejected) < retained_gate_compact.index(accepted)
            and retained_gate_compact.index(accepted) < retained_gate_compact.index(recheck)
            and retained_gate_compact.index(recheck) < retained_gate_compact.index(call),
            "FSR accept/reject and retained-domain entry boundaries are flushed in order")

    profile_fields = [
        "RemoteStorageRegistered",
        "RemoteStorageReady",
        "ServiceRegistryRegistered",
        "IpaServerRegistered",
        "LinuxIpaClientEnabled",
        "TftpPort",
        "TftpServerPrepared",
    ]
    profile_reader = function_body(
        qcom, "pub fn runtime_profile_report_with("
    )
    profile_reader_compact = compact(profile_reader)
    for field in profile_fields:
        require(
            f"checkpoint(RuntimeProfileField::{field},None);" in profile_reader_compact
            and f"checkpoint(RuntimeProfileField::{field},Some(" in profile_reader_compact,
            "runtime profile reader brackets the " + field + " read with checkpoints",
        )
        require(f"Field::{field} =>" in gate,
                "display handles runtime profile field " + field)
    require(gate.count("d.flush();") >= 17,
            "health, profile, and final gate results are individually flushed")
    require('"GATES PASS; STOP HERE"' in gate
            and '"STOP BEFORE TAKE/PAS4"' in gate,
            "all-pass diagnostic still stops before runtime handoff/PAS4")
    require('"NO RESET/GLINK/WLAN"' in gate
            and '"NO FLASH/RETRY"' in gate,
            "diagnostic screen states the remaining no-start/no-flash boundary")

    runtime_boot_report = function_body(
        rmtfs,
        "pub(super) fn runtime_boot_report(&self) -> RuntimeBootReport",
    )
    require("runtime_profile_report" not in runtime_boot_report
            and "preboot_generation" not in runtime_boot_report,
            "factory snapshot leaves the potentially trapping profile read separate")
    boot_reader = function_body(
        rmtfs, "pub(super) fn runtime_boot_report_with("
    )
    boot_fields = [
        "Phase", "ServerPresent", "ServerFailed", "SessionPresent",
        "TftpProviderPresent",
    ]
    boot_reader_compact = compact(boot_reader)
    for field in boot_fields:
        require(
            f"checkpoint(RuntimeBootField::{field},None);" in boot_reader_compact
            and f"checkpoint(RuntimeBootField::{field},Some(" in boot_reader_compact,
            "factory snapshot brackets the " + field + " read with checkpoints",
        )
        require(f"Field::{field} =>" in gate,
                "display handles factory field " + field)
    require("runtime_boot_report_with(" in gate,
            "diagnostic displays the factory fields as they are read")
    require("pub(super) fn runtime_profile_report(" in rmtfs,
            "profile report has a separately callable read boundary")
    require(
        f'#[cfg(all(feature="sunfish-native-startup-mpss-runtime",not(feature="{DIAGNOSTIC}")))]'
        in compact(rmtfs),
        "mutable runtime handoff is not compiled into the diagnostic feature",
    )
    require(
        f'#[cfg(not(feature="{DIAGNOSTIC}"))]#[inline(never)]fnstart_runtime_after_gate'
        in compact(auth),
        "PAS loader loop is excluded from the diagnostic feature",
    )
    require(
        re.search(
            rf'#\[cfg\(not\(feature\s*=\s*"{DIAGNOSTIC}"\)\)\]\s*'
            r'const MODEM:\s*&\[u8\]\s*=\s*include_bytes!',
            auth,
        ) is not None,
        "MPSS firmware inclusion is cfg-disabled in the diagnostic feature",
    )
    for forbidden in (
        "MODEM_DEFAULT", "--modem-firmware", "private-modem.mbn",
        "SUNFISH_MPSS_FIRMWARE_PATH",
    ):
        require(forbidden not in builder,
                "diagnostic builder does not package MPSS firmware: " + forbidden)

    irq = function_body(
        startup,
        "fn notified(&mut self, channels: ChannelSet) -> Result<(), Self::Error>",
    )
    irq_compact = compact(irq)
    require(
        "!pixel_mpss_auth::diagnostics_owned()&&pixel_mpss_preauth::started()"
        in irq_compact,
        "preauth painter is suppressed after auth diagnostics own the display",
    )
    require(
        '#[cfg(all(feature="sunfish-native-startup-mpss-preauth",feature="sunfish-native-startup-mpss-runtime",not(feature="sunfish-native-startup-mpss-runtime-gate-diagnostic")))]if!pixel_mpss_auth::diagnostics_owned()&&pixel_mpss_preauth::started()' in irq_compact,
        "ordinary runtime keeps its existing preauth notification painter",
    )
    receive_notified = function_body(
        receive, "pub(super) fn notified(state: &mut Startup, channels: ChannelSet)",
    )
    receive_compact = compact(receive_notified)
    require(
        'if!super::diagnostic_display_owned(){ifletSome(r)=state._irq_registers{show(' in receive_compact,
        "IPA receive IRQ preserves the diagnostic screen and normal receive display",
    )
    for name, source in irq_painters.items():
        body = compact(function_body(
            source, "pub(super) fn late_notification(state: &mut Startup)",
        ))
        require(
            "ifsuper::diagnostic_display_owned(){return;}" in body,
            name + " late IRQ path preserves diagnostic display ownership",
        )
    stop_on_fault = compact(function_body(smmu, "pub(crate) fn stop_on_fault(&mut self)"))
    require(
        f'#[cfg(feature="{DIAGNOSTIC}")]ifsuper::diagnostic_display_owned(){{return;}}' in stop_on_fault
        and stop_on_fault.index("ifsuper::diagnostic_display_owned(){return;}")
        < stop_on_fault.index("self.display.replace_line"),
        "SMMU/domain fault IRQ retains fault handling without replacing the report and leaves standalone probe builds unchanged",
    )
    for signature in (
        "fn dispatch_uc(state: &mut Startup, external_fault: bool)",
        "fn contain_irq(state: &mut Startup)",
    ):
        body = compact(function_body(startup, signature))
        require(
            "ifdiagnostic_display_owned(){return;}" in body,
            signature + " IRQ fallback preserves diagnostic display ownership",
        )
    require(
        'letdiagnostic_display_owned=if' not in compact(startup)
        and 'returnpixel_mpss_auth::diagnostics_owned();' in compact(startup)
        and '#[cfg(not(feature="sunfish-native-startup-mpss-runtime"))]{false}' in compact(startup),
        "diagnostic display ownership follows the runtime auth-attempt marker",
    )

    paths = [
        ROOT / "rust/Cargo.toml", auth_path, rmtfs_path, qcom_path,
        startup_path, receive_path, smmu_path, native_smmu_path, *irq_painter_paths.values(),
        builder_path, PORT / "build-native-startup-preflight.sh",
    ]
    result = {
        "feature": DIAGNOSTIC,
        "diagnostic_attempt_marker_before_health": True,
        "health_checkpoints_flushed": True,
        "profile_read_start_flushed_before_call": True,
        "profile_fields_flushed_before_and_after_each_read": True,
        "factory_fields_flushed_before_and_after_each_read": True,
        "preauth_painter_suppressed_after_attempt": True,
        "ipa_receive_painter_suppressed_after_attempt": True,
        "all_ipa_irq_painters_suppressed_after_attempt": True,
        "smmu_fault_painter_suppressed_after_attempt": True,
        "handoff_and_pas4_stopped_in_diagnostic_feature": True,
        "source_guard_reports_labeled_failures_and_continues_on_pass": True,
        "mpss_firmware_embedded": False,
        "source_sha256": {
            str(path.resolve()): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in paths
        },
    }
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        args.output.write_text(json.dumps(result, indent=2) + "\n")
    print("PASS: runtime gate diagnostic checkpoints and stop boundary are source-audited.")


if __name__ == "__main__":
    main()
