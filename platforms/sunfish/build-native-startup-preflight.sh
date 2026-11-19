#!/bin/sh
set -eu
umask 077
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
mode="${1:-preflight}"
case "$mode" in
    preflight) feature="sunfish-native-startup-preflight"; profile="native-startup-preflight" ;;
    hardware-config) feature="sunfish-native-startup-ipa-config"; profile="native-startup-ipa-config" ;;
    dma-domain) feature="sunfish-native-startup-ipa-domain"; profile="native-startup-ipa-domain" ;;
    local-memory) feature="sunfish-native-startup-ipa-memory"; profile="native-startup-ipa-memory" ;;
    buffers) feature="sunfish-native-startup-ipa-buffers"; profile="native-startup-ipa-buffers" ;;
    resources) feature="sunfish-native-startup-ipa-resources"; profile="native-startup-ipa-resources" ;;
    irq) feature="sunfish-native-startup-ipa-irq"; profile="native-startup-ipa-irq" ;;
    shared) feature="sunfish-native-startup-ipa-shared"; profile="native-startup-ipa-shared" ;;
    uc) feature="sunfish-native-startup-ipa-uc"; profile="native-startup-ipa-shared" ;;
    firmware) feature="sunfish-native-startup-ipa-firmware"; profile="native-startup-ipa-firmware" ;;
    gsi) feature="sunfish-native-startup-ipa-gsi"; profile="native-startup-ipa-gsi" ;;
    command) feature="sunfish-native-startup-ipa-command"; profile="native-startup-ipa-command" ;;
    transfer) feature="sunfish-native-startup-ipa-transfer"; profile="native-startup-ipa-transfer" ;;
    tables) feature="sunfish-native-startup-ipa-tables"; profile="native-startup-ipa-tables" ;;
    receive-memory) feature="sunfish-native-startup-ipa-receive-memory"; profile="native-startup-ipa-receive" ;;
    receive) feature="sunfish-native-startup-ipa-receive"; profile="native-startup-ipa-receive" ;;
    rmtfs) feature="sunfish-native-startup-linux-rmtfs"; profile="native-startup-linux-rmtfs" ;;
    mpss-smem) feature="sunfish-native-startup-mpss-smem"; profile="native-startup-mpss-smem" ;;
    mpss-preauth) feature="sunfish-native-startup-mpss-preauth"; profile="native-startup-mpss-preauth" ;;
    mpss-auth) feature="sunfish-native-startup-mpss-auth"; profile="native-startup-mpss-auth" ;;
    mpss-runtime) feature="sunfish-native-startup-mpss-runtime"; profile="native-startup-mpss-runtime" ;;
    mpss-runtime-gate-diagnostic) feature="sunfish-native-startup-mpss-runtime-gate-diagnostic"; profile="native-startup-mpss-runtime" ;;
    mpss-runtime-prescm-diagnostic) feature="sunfish-native-startup-mpss-runtime-prescm-diagnostic"; profile="native-startup-mpss-runtime" ;;
    mpss-runtime-share-diagnostic) feature="sunfish-native-startup-mpss-runtime-share-diagnostic"; profile="native-startup-mpss-runtime" ;;
    mpss-runtime-share-trace) feature="sunfish-native-startup-mpss-runtime-share-trace"; profile="native-startup-mpss-runtime" ;;
    mpss-source-diagnostic) feature="sunfish-native-startup-mpss-auth"; profile="native-startup-mpss-auth-source-diagnostic-v6"; system_profile="native-startup-mpss-auth" ;;
    *) printf 'Expected preflight, hardware-config, dma-domain, local-memory, buffers, resources, irq, shared, uc, firmware, gsi, command, transfer, tables, receive-memory, receive, rmtfs, mpss-smem/mpss-preauth, mpss-auth, mpss-runtime, mpss-runtime-gate-diagnostic, mpss-runtime-prescm-diagnostic or mpss-source-diagnostic\n' >&2; exit 2 ;;
esac
qrtr_flow_experiment=false
if [ "${SUNFISH_NATIVE_QRTR_FLOW_EXPERIMENT:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ]; then
        printf 'QRTR flow experiment requires mpss-runtime mode\n' >&2
        exit 2
    fi
    qrtr_flow_experiment=true
    feature="$feature,sunfish-qrtr-rmtfs-flow-experiment"
fi
qrtr_deferred_requests=false
if [ "${SUNFISH_NATIVE_QRTR_DEFERRED_REQUESTS:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ] || [ "$qrtr_flow_experiment" != true ]; then
        printf 'Deferred QRTR requests require mpss-runtime and the QRTR flow experiment\n' >&2
        exit 2
    fi
    qrtr_deferred_requests=true
    feature="$feature,sunfish-qrtr-deferred-requests"
fi
ipa_postmodem_reference=false
if [ "${SUNFISH_NATIVE_IPA_POSTMODEM_REFERENCE:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ] || [ "$qrtr_deferred_requests" != true ]; then
        printf 'Post-modem IPA reference experiment requires mpss-runtime and deferred QRTR requests\n' >&2
        exit 2
    fi
    ipa_postmodem_reference=true
    feature="$feature,sunfish-ipa-postmodem-reference-experiment"
fi
ipa_qrtr_flow_experiment=false
if [ "${SUNFISH_NATIVE_IPA_QRTR_FLOW_EXPERIMENT:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ] || [ "$qrtr_deferred_requests" != true ]; then
        printf 'IPA QRTR flow experiment requires mpss-runtime and deferred QRTR requests\n' >&2
        exit 2
    fi
    ipa_qrtr_flow_experiment=true
    feature="$feature,sunfish-qrtr-ipa-flow-experiment"
fi
servreg_qrtr_flow_experiment=false
if [ "${SUNFISH_NATIVE_SERVREG_QRTR_FLOW_EXPERIMENT:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ] || [ "$qrtr_deferred_requests" != true ]; then
        printf 'SERVREG QRTR flow experiment requires mpss-runtime and deferred QRTR requests\n' >&2
        exit 2
    fi
    servreg_qrtr_flow_experiment=true
    feature="$feature,sunfish-qrtr-servreg-flow-experiment"
fi
ipa_ignore_config_reference=false
if [ "${SUNFISH_NATIVE_IPA_IGNORE_CONFIG_REFERENCE:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ] || [ "$servreg_qrtr_flow_experiment" != true ]; then
        printf 'IPA config reference behavior requires mpss-runtime and SERVREG flow experiment\n' >&2
        exit 2
    fi
    ipa_ignore_config_reference=true
    feature="$feature,sunfish-ipa-ignore-config-reference"
fi
ipa_ignore_unhandled_reference=false
if [ "${SUNFISH_NATIVE_IPA_IGNORE_UNHANDLED_REFERENCE:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ] || [ "$ipa_ignore_config_reference" != true ]; then
        printf 'Unhandled IPA reference behavior requires mpss-runtime and IPA config reference behavior\n' >&2
        exit 2
    fi
    ipa_ignore_unhandled_reference=true
    feature="$feature,sunfish-ipa-ignore-unhandled-reference"
fi
ipa_ignore_init_response_reference=false
if [ "${SUNFISH_NATIVE_IPA_IGNORE_INIT_RESPONSE_REFERENCE:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ] || [ "$ipa_ignore_unhandled_reference" != true ]; then
        printf 'IPA init response reference behavior requires mpss-runtime and unhandled IPA reference behavior\n' >&2
        exit 2
    fi
    ipa_ignore_init_response_reference=true
    feature="$feature,sunfish-ipa-ignore-init-response-reference"
fi
tftp_observer=false
if [ "${SUNFISH_NATIVE_TFTP_OBSERVER:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ] || [ "$ipa_ignore_init_response_reference" != true ]; then
        printf 'TFTP observer requires mpss-runtime and the IPA init response reference mode\n' >&2
        exit 2
    fi
    tftp_observer=true
    feature="$feature,sunfish-native-tftp-observer"
fi
tftp_server_check=false
if [ "${SUNFISH_NATIVE_TFTP_SERVER_CHECK:-0}" = 1 ]; then
    if [ "$mode" != mpss-runtime ] || [ "$ipa_ignore_init_response_reference" != true ] || [ "$tftp_observer" = true ]; then
        printf 'TFTP server-check requires mpss-runtime and IPA reference mode, without the observer\n' >&2
        exit 2
    fi
    tftp_server_check=true
    feature="$feature,sunfish-native-tftp-server-check"
fi
runtime_mode=false
if [ "$mode" = mpss-runtime ] || [ "$mode" = mpss-runtime-gate-diagnostic ] || [ "$mode" = mpss-runtime-prescm-diagnostic ] || [ "$mode" = mpss-runtime-share-diagnostic ] || [ "$mode" = mpss-runtime-share-trace ]; then runtime_mode=true; fi
system_profile="${system_profile:-$profile}"
source_diagnostic=false
if [ "$mode" = mpss-source-diagnostic ] || [ "${SUNFISH_SOURCE_GUARD_DIAGNOSTIC:-0}" = 1 ]; then source_diagnostic=true; fi
if [ "${SUNFISH_SOURCE_GUARD_DIAGNOSTIC:-0}" = 1 ] && [ "$mode" != mpss-source-diagnostic ]; then
    printf 'Source diagnostic requires the separate mpss-source-diagnostic output profile\n' >&2
    exit 2
fi
mpss_stage=false
if [ "$mode" = mpss-smem ] || [ "$mode" = mpss-preauth ] || [ "$mode" = mpss-auth ] || [ "$runtime_mode" = true ] || [ "$mode" = mpss-source-diagnostic ]; then mpss_stage=true; fi
firmware_stage=false
gsi_stage=false
if [ "$mode" = gsi ] || [ "$mode" = command ] || [ "$mode" = transfer ] || [ "$mode" = tables ] || [ "$mode" = receive-memory ] || [ "$mode" = receive ] || [ "$mode" = rmtfs ] || [ "$mpss_stage" = true ]; then gsi_stage=true; fi
if [ "$mode" = firmware ] || [ "$gsi_stage" = true ]; then firmware_stage=true; fi
checks_mode="$mode"
if [ "$mode" = shared ] || [ "$mode" = uc ] || [ "$firmware_stage" = true ]; then checks_mode=irq; fi
out="${SUNFISH_NATIVE_OUTPUT_DIR:-$root_dir/.cache/sunfish-native-wifi/private/$profile}"
if [ "$mode" = mpss-runtime-prescm-diagnostic ]; then
    out="${SUNFISH_NATIVE_OUTPUT_DIR:-$root_dir/.cache/sunfish-native-wifi/private/native-startup-mpss-runtime-prescm-diagnostic}"
fi
if [ "$mode" = mpss-source-diagnostic ]; then
    for marker in manifest.json attempt.json precompile-inputs.json cargo-build.log fastboot.img; do
        if [ -e "$out/$marker" ]; then
            printf 'Refusing to reuse an existing source-diagnostic output directory: %s\n' "$out" >&2
            exit 2
        fi
    done
fi
if [ "$mode" = uc ]; then
    out="${SUNFISH_NATIVE_OUTPUT_DIR:-$root_dir/.cache/sunfish-native-wifi/private/native-startup-ipa-uc}"
fi
if [ "$mode" = command ]; then
    out="${SUNFISH_NATIVE_OUTPUT_DIR:-$root_dir/.cache/sunfish-native-wifi/private/native-startup-ipa-command}"
fi
if [ "$mode" = receive-memory ]; then
    out="${SUNFISH_NATIVE_OUTPUT_DIR:-$root_dir/.cache/sunfish-native-wifi/private/native-startup-ipa-receive-memory}"
fi
inputs="${SUNFISH_PRIVATE_INPUT_DIR:?Select the prepared private input directory}"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="${SUNFISH_REFERENCE_BOOT:-$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img}"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
mkdir -p "$out"
cd "$root_dir"
python3 - "$out/build-profile.json" "$mode" "$feature" "$profile" "$system_profile" "$qrtr_flow_experiment" "$qrtr_deferred_requests" "$ipa_postmodem_reference" "$ipa_qrtr_flow_experiment" "$servreg_qrtr_flow_experiment" "$ipa_ignore_config_reference" "$ipa_ignore_unhandled_reference" "$ipa_ignore_init_response_reference" "$tftp_observer" "$tftp_server_check" <<'PY'
import json,sys
from pathlib import Path
out,mode,feature,profile,system_profile,qrtr_flow_experiment,qrtr_deferred_requests,ipa_postmodem_reference,ipa_qrtr_flow_experiment,servreg_qrtr_flow_experiment,ipa_ignore_config_reference,ipa_ignore_unhandled_reference,ipa_ignore_init_response_reference,tftp_observer,tftp_server_check=sys.argv[1:]
Path(out).write_text(json.dumps({
    'mode':mode,
    'feature':feature,
    'profile':profile,
    'system_profile':system_profile,
    'gate_diagnostic':mode=='mpss-runtime-gate-diagnostic',
    'prescm_diagnostic':mode=='mpss-runtime-prescm-diagnostic',
    'qrtr_flow_experiment':qrtr_flow_experiment=='true',
    'qrtr_deferred_requests':qrtr_deferred_requests=='true',
    'ipa_postmodem_reference':ipa_postmodem_reference=='true',
    'ipa_qrtr_flow_experiment':ipa_qrtr_flow_experiment=='true',
    'servreg_qrtr_flow_experiment':servreg_qrtr_flow_experiment=='true',
    'ipa_ignore_config_reference':ipa_ignore_config_reference=='true',
    'ipa_ignore_unhandled_reference':ipa_ignore_unhandled_reference=='true',
    'ipa_ignore_init_response_reference':ipa_ignore_init_response_reference=='true',
    'tftp_observer':tftp_observer=='true',
    'tftp_server_check':tftp_server_check=='true',
    'loader_source_guard':('labeled-pass-through' if mode in ('mpss-runtime','mpss-runtime-gate-diagnostic','mpss-runtime-prescm-diagnostic') else 'native-startup-entry'),
    'device_actions':[],
},indent=2)+'\n')
PY
if [ "$mode" = mpss-auth ] || [ "$runtime_mode" = true ] || [ "$mode" = mpss-source-diagnostic ]; then
    if [ "$source_diagnostic" = true ]; then
        export SUNFISH_SOURCE_GUARD_DIAGNOSTIC=1
        python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out" --checkpoints --native-source-diagnostic > "$out/loader-sdk-build.log" 2>&1
    elif [ "$runtime_mode" = true ]; then
        python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out" --checkpoints --native-source-report-guard > "$out/loader-sdk-build.log" 2>&1
    else
        python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out" --checkpoints --native-startup-entry > "$out/loader-sdk-build.log" 2>&1
    fi
    sdk="$out/sdk"
    mpss_source="${SUNFISH_MPSS_FIRMWARE:-$root_dir/.cache/sunfish-mobian-comparison/wifi-preparation/private/firmware/qcom/sm7150/google/sunfish/modem.mbn}"
    python3 - "$mpss_source" "$out/private-modem.mbn" <<'PY'
import hashlib,sys,shutil
from pathlib import Path
p=Path(sys.argv[1]);blob=p.read_bytes()
if len(blob)!=68460672 or hashlib.sha256(blob).hexdigest()!='8489e6828dc12ed98e26dec15adb7102a31aafaf7314e249854c737ddba4cdc0':
    raise SystemExit('Wrong private MPSS reference identity')
shutil.copyfile(p,sys.argv[2])
print('Verified exact private MPSS input; no device operation.')
PY
    export SUNFISH_MPSS_FIRMWARE_PATH="$out/private-modem.mbn"
fi
if [ "$firmware_stage" = true ]; then
    ipa_source="${SUNFISH_IPA_FIRMWARE:-$root_dir/.cache/sunfish-mobian-comparison/wifi-preparation/private/firmware/qcom/sm7150/google/sunfish/ipa_fws.mbn}"
    python3 - "$ipa_source" "$out/private-ipa_fws.mbn" <<'PY'
import hashlib,sys,shutil
from pathlib import Path
p=Path(sys.argv[1]);blob=p.read_bytes()
if len(blob)!=29120 or hashlib.sha256(blob).hexdigest()!='7d61b64288248719d14a4b316e9cb83917c11a2c9753dff5475a7a2db95394dd':
    raise SystemExit('Wrong private IPA reference identity')
shutil.copyfile(p,sys.argv[2])
print('Verified exact private IPA input; no device operation.')
PY
    export SUNFISH_IPA_FIRMWARE_PATH="$out/private-ipa_fws.mbn"
fi
skip_tests=false
if { [ "$mode" = mpss-runtime-prescm-diagnostic ] || [ "$mode" = mpss-runtime-share-diagnostic ] || [ "$mode" = mpss-runtime-share-trace ]; } && [ "${SUNFISH_SKIP_NATIVE_TESTS:-0}" = 1 ]; then skip_tests=true; fi
if [ "$skip_tests" = false ] && [ "$firmware_stage" = true ]; then
    if [ "$gsi_stage" = true ]; then
        python3 "$port_dir/test-native-ipa-linux-gsi.py" > "$out/gsi-tests.log"
    fi
    if [ "$mode" = receive-memory ] || [ "$mode" = receive ] || [ "$mode" = rmtfs ] || [ "$mpss_stage" = true ]; then
        python3 "$port_dir/test-native-ipa-linux-transfer.py" > "$out/transfer-tests.log"
        python3 "$port_dir/test-native-ipa-linux-receive-native.py" > "$out/receive-native-tests.log"
        python3 "$port_dir/test-native-ipa-linux-receive-domain.py" > "$out/receive-domain-tests.log"
        python3 "$port_dir/test-native-ipa-linux-receive-install.py" > "$out/receive-install-tests.log"
        if [ "$mode" = receive ] || [ "$mode" = rmtfs ] || [ "$mpss_stage" = true ]; then
            python3 "$port_dir/test-native-ipa-receive-policy.py" > "$out/receive-policy-tests.log"
            python3 "$port_dir/audit-native-ipa-receive-source.py" --output "$out/receive-source-audit.json"
        else
            python3 "$port_dir/audit-native-ipa-receive-memory-source.py" --output "$out/receive-source-audit.json"
        fi
        for source_kind in table transfer command gsi linux; do
            cp "$out/receive-source-audit.json" "$out/$source_kind-source-audit.json"
        done
    elif [ "$mode" = tables ]; then
        python3 "$port_dir/test-native-ipa-linux-transfer.py" > "$out/transfer-tests.log"
        python3 "$port_dir/audit-native-ipa-tables-source.py" --output "$out/table-source-audit.json"
        cp "$out/table-source-audit.json" "$out/transfer-source-audit.json"
        cp "$out/table-source-audit.json" "$out/command-source-audit.json"
        cp "$out/table-source-audit.json" "$out/gsi-source-audit.json"
        cp "$out/table-source-audit.json" "$out/linux-source-audit.json"
    elif [ "$mode" = transfer ]; then
        python3 "$port_dir/test-native-ipa-linux-transfer.py" > "$out/transfer-tests.log"
        python3 "$port_dir/audit-native-ipa-transfer-source.py" --output "$out/transfer-source-audit.json"
        cp "$out/transfer-source-audit.json" "$out/command-source-audit.json"
        cp "$out/transfer-source-audit.json" "$out/gsi-source-audit.json"
        cp "$out/transfer-source-audit.json" "$out/linux-source-audit.json"
    elif [ "$mode" = command ]; then
        python3 "$port_dir/test-native-ipa-linux-command.py" > "$out/command-tests.log"
        python3 "$port_dir/audit-native-ipa-command-source.py" --output "$out/command-source-audit.json"
        # The most-derived source audit includes GSI, PAS and UC checks.
        cp "$out/command-source-audit.json" "$out/gsi-source-audit.json"
        cp "$out/command-source-audit.json" "$out/linux-source-audit.json"
    elif [ "$mode" = gsi ]; then
        python3 "$port_dir/audit-native-ipa-gsi-source.py" --output "$out/gsi-source-audit.json"
        cp "$out/gsi-source-audit.json" "$out/linux-source-audit.json"
    else
        python3 "$port_dir/audit-native-ipa-firmware-source.py" --output "$out/linux-source-audit.json"
    fi
    rustc --edition 2024 -D warnings --test tests/sunfish_ipa_linux_loader_test.rs -o "$out/loader-test"
    "$out/loader-test" --quiet > "$out/loader-tests.log"
    SUNFISH_IPA_TEST_FIRMWARE="$out/private-ipa_fws.mbn" "$out/loader-test" --ignored --nocapture > "$out/private-image-tests.log"
    rustc --edition 2024 -D warnings --test tests/sunfish_scm_linux_power_test.rs -o "$out/scm-test"
    "$out/scm-test" --quiet > "$out/scm-tests.log"
fi
if [ "$skip_tests" = false ] && { [ "$mode" = shared ] || [ "$mode" = uc ] || [ "$firmware_stage" = true ]; }; then
    if [ "$mode" = uc ]; then
        python3 "$port_dir/audit-native-ipa-uc-source.py" --output "$out/linux-source-audit.json"
    fi
    if [ "$mode" = uc ] || [ "$firmware_stage" = true ]; then
        rustc --edition 2024 -D warnings --test tests/sunfish_ipa_linux_uc_test.rs -o "$out/uc-test"
        "$out/uc-test" --quiet > "$out/uc-tests.log"
    else
        python3 "$port_dir/audit-native-ipa-shared-source.py" --output "$out/linux-source-audit.json"
    fi
    rustc --edition 2024 -D warnings --test tests/sunfish_ipa_shared_memory_test.rs -o "$out/shared-test"
    "$out/shared-test" --quiet > "$out/shared-tests.log"
elif [ "$skip_tests" = false ]; then
    python3 "$port_dir/audit-native-ipa-linux-source.py" --output "$out/linux-source-audit.json"
fi
if [ "$skip_tests" = false ] && { [ "$mode" = rmtfs ] || [ "$mpss_stage" = true ]; }; then
    python3 "$port_dir/test-native-linux-rmtfs-server.py" > "$out/rmtfs-server-tests.log"
    python3 "$port_dir/audit-native-linux-rmtfs-source.py" --output "$out/rmtfs-source-audit.json"
fi
if [ "$skip_tests" = false ] && [ "$mpss_stage" = true ]; then
    python3 "$port_dir/test-native-mpss-linux-smem.py" > "$out/mpss-smem-tests.log"
    python3 "$port_dir/test-native-mpss-smem-entry.py" > "$out/mpss-smem-entry-tests.log"
    python3 "$port_dir/audit-native-mpss-smem-source.py" --output "$out/mpss-smem-source-audit.json"
fi
if [ "$skip_tests" = false ] && { [ "$mode" = mpss-preauth ] || [ "$mode" = mpss-auth ] || [ "$runtime_mode" = true ] || [ "$mode" = mpss-source-diagnostic ]; }; then
    python3 "$port_dir/test-native-mpss-linux-power.py" > "$out/mpss-power-tests.log"
    python3 "$port_dir/test-native-mpss-linux-smp2p.py" > "$out/mpss-smp2p-tests.log"
    python3 "$port_dir/test-native-mpss-preauth-entry.py" > "$out/mpss-preauth-entry-tests.log"
    python3 "$port_dir/audit-native-mpss-preauth-source.py" --output "$out/mpss-preauth-source-audit.json"
fi
if [ "$skip_tests" = false ] && { [ "$mode" = mpss-auth ] || [ "$runtime_mode" = true ] || [ "$mode" = mpss-source-diagnostic ]; }; then
    python3 "$port_dir/test-native-mpss-linux-boot.py" > "$out/mpss-boot-tests.log"
    python3 "$port_dir/test-native-mpss-auth-root.py" > "$out/mpss-auth-root-tests.log"
    python3 "$port_dir/audit-native-mpss-auth-source.py" --output "$out/mpss-auth-source-audit.json"
fi
if [ "$skip_tests" = false ] && [ "$runtime_mode" = true ]; then
    rustc --edition=2024 -D warnings --test tests/pixel_mpss_runtime_diagnostic_test.rs -o "$out/runtime-diagnostic-test"
    "$out/runtime-diagnostic-test" --quiet > "$out/runtime-diagnostic-tests.log"
    python3 "$port_dir/test-native-mpss-runtime-services.py" > "$out/mpss-runtime-service-tests.log"
    python3 "$port_dir/test-native-mpss-runtime-irq.py" > "$out/mpss-runtime-irq-tests.log"
    python3 "$port_dir/test-native-mpss-runtime-source.py" > "$out/mpss-runtime-source-tests.log"
    python3 "$port_dir/audit-native-mpss-runtime-source.py" --workspace "$root_dir" --output "$out/runtime-source-audit.json"
    if [ "$qrtr_flow_experiment" = true ]; then
        python3 "$port_dir/test-native-qrtr-flow.py" > "$out/qrtr-flow-tests.log"
    fi
    if [ "$qrtr_deferred_requests" = true ]; then
        python3 "$port_dir/test-native-qrtr-deferred.py" > "$out/qrtr-deferred-tests.log"
    fi
    if [ "$ipa_qrtr_flow_experiment" = true ]; then
        python3 "$port_dir/test-native-ipa-qrtr-flow.py" > "$out/ipa-qrtr-flow-tests.log"
    fi
    if [ "$servreg_qrtr_flow_experiment" = true ]; then
        python3 "$port_dir/test-native-servreg-qrtr-flow.py" > "$out/servreg-qrtr-flow-tests.log"
    fi
    if [ "$ipa_ignore_config_reference" = true ]; then
        python3 "$port_dir/test-native-ipa-ignore-config.py" > "$out/ipa-ignore-config-tests.log"
    fi
    if [ "$ipa_ignore_unhandled_reference" = true ]; then
        python3 "$port_dir/test-native-ipa-ignore-config.py" --unhandled > "$out/ipa-ignore-unhandled-tests.log"
    fi
    if [ "$ipa_ignore_init_response_reference" = true ]; then
        python3 "$port_dir/test-native-ipa-ignore-config.py" --unhandled --init-response > "$out/ipa-ignore-init-response-tests.log"
    fi
fi
python3 "$port_dir/verify-native-boot-inputs.py" --inputs "$inputs"
if [ "$skip_tests" = false ]; then
rustc --edition 2024 -D warnings --test tests/sunfish_ipa_linux_power_test.rs -o "$out/power-test"
"$out/power-test" --quiet > "$out/power-tests.log"
rustc --edition 2024 -D warnings --test tests/sunfish_ipa_linux_config_test.rs -o "$out/config-test"
"$out/config-test" --quiet > "$out/config-tests.log"
if [ "$checks_mode" = dma-domain ] || [ "$checks_mode" = local-memory ] || [ "$checks_mode" = buffers ] || [ "$checks_mode" = resources ] || [ "$checks_mode" = irq ]; then
    rustc --edition 2024 -D warnings --test tests/sunfish_smmu_native_test.rs -o "$out/smmu-test"
    "$out/smmu-test" --quiet > "$out/smmu-tests.log"
fi
if [ "$checks_mode" = local-memory ] || [ "$checks_mode" = buffers ] || [ "$checks_mode" = resources ] || [ "$checks_mode" = irq ]; then
    rustc --edition 2024 -D warnings --test tests/sunfish_ipa_linux_memory_test.rs -o "$out/memory-test"
    "$out/memory-test" --quiet > "$out/memory-tests.log"
fi
if [ "$checks_mode" = buffers ] || [ "$checks_mode" = resources ] || [ "$checks_mode" = irq ]; then
    rustc --edition 2024 -D warnings --test tests/sunfish_ipa_linux_buffers_test.rs -o "$out/buffer-test"
    "$out/buffer-test" --quiet > "$out/buffer-tests.log"
fi
if [ "$checks_mode" = resources ] || [ "$checks_mode" = irq ]; then
    rustc --edition 2024 -D warnings --test tests/sunfish_ipa_linux_resources_test.rs -o "$out/resource-test"
    "$out/resource-test" --quiet > "$out/resource-tests.log"
fi
if [ "$checks_mode" = irq ]; then
    rustc --edition 2024 -D warnings --test tests/sunfish_ipa_linux_irq_test.rs -o "$out/irq-test"
    "$out/irq-test" --quiet > "$out/irq-tests.log"
fi
fi
export SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include"
export CARGO_TARGET_DIR="$root_dir/.cache/sunfish-native-wifi/private/preflight-target"
export SUNFISH_PRIVATE_BOOT_INPUTS_RS_PATH="$inputs/private_boot_inputs.rs"
if [ "$mode" = mpss-auth ] || [ "$mode" = mpss-source-diagnostic ]; then
    python3 "$port_dir/snapshot-native-mpss-auth-inputs.py" --output-dir "$out" --inputs "$inputs"
fi
if ! cargo +nightly-2026-09-25 build --locked --offline --release --manifest-path rust/Cargo.toml \
    --features "$feature" --bin rust_sunfish_native_startup_preflight \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json -Z build-std=core,alloc -Z json-target-spec > "$out/cargo-build.log" 2>&1; then
    tail -30 "$out/cargo-build.log"
    exit 1
fi
cp "$CARGO_TARGET_DIR/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_native_startup_preflight.elf" "$out/native_startup_preflight.elf"
"$sdk/bin/microkit" "$port_dir/$system_profile.system" --search-path "$out" --board sunfish --config debug \
    -o "$out/loader.img" -r "$out/report.txt" --capdl-json "$out/capdl.json"
if [ "$runtime_mode" = true ]; then
    observer_audit_arg=""
    if [ "$tftp_observer" = true ]; then observer_audit_arg="--tftp-observer"; fi
    if [ "$tftp_server_check" = true ]; then observer_audit_arg="--tftp-server-check"; fi
    python3 "$port_dir/validate-native-mpss-runtime.py" --system "$port_dir/$system_profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json" $observer_audit_arg
elif [ "$mode" = mpss-auth ] || [ "$mode" = mpss-source-diagnostic ]; then
    python3 "$port_dir/validate-native-mpss-auth.py" --system "$port_dir/$system_profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$mode" = mpss-preauth ]; then
    python3 "$port_dir/validate-native-mpss-preauth.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$mpss_stage" = true ]; then
    python3 "$port_dir/validate-native-mpss-smem.py" --system "$port_dir/$system_profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$mode" = rmtfs ]; then
    python3 "$port_dir/validate-native-linux-rmtfs.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$mode" = receive ]; then
    python3 "$port_dir/validate-native-ipa-receive.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$mode" = receive-memory ]; then
    python3 "$port_dir/validate-native-ipa-receive-memory.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$mode" = tables ]; then
    python3 "$port_dir/validate-native-ipa-tables.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$mode" = transfer ]; then
    python3 "$port_dir/validate-native-ipa-transfer.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$mode" = command ]; then
    python3 "$port_dir/validate-native-ipa-command.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$gsi_stage" = true ]; then
    python3 "$port_dir/validate-native-ipa-gsi.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$firmware_stage" = true ]; then
    python3 "$port_dir/validate-native-ipa-firmware.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
elif [ "$mode" = shared ] || [ "$mode" = uc ]; then
    python3 "$port_dir/validate-native-ipa-shared.py" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
else
    python3 "$port_dir/validate-native-startup-preflight.py" --profile "$mode" --system "$port_dir/$profile.system" --capdl "$out/capdl.json" --report "$out/report.txt" \
        --dt "$root_dir/.cache/pixel-sunfish/live-runtime.dtb" \
        --dt "$root_dir/.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/build/kernel.dtb" --output "$out/capability-audit.json"
fi
python3 "$port_dir/wrap-microkit-image.py" "$out/loader.img" "$out/native-startup-preflight.Image"
lz4 -z -f -q "$out/native-startup-preflight.Image" "$out/native-startup-preflight.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" "$reference" "$out/native-startup-preflight.Image.lz4" "$out/fastboot.img"
if [ "$mode" = mpss-auth ] || [ "$runtime_mode" = true ] || [ "$mode" = mpss-source-diagnostic ]; then
    python3 "$port_dir/validate-native-mpss-auth-boot.py" --output-dir "$out" --reference "$reference"
fi
shasum -a 256 "$out/fastboot.img" > "$out/fastboot.img.sha256"
printf 'Built private native IPA prestart image: %s\n' "$out/fastboot.img"
