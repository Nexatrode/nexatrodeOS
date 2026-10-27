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
    mpss-source-diagnostic) feature="sunfish-native-startup-mpss-auth"; profile="native-startup-mpss-auth-source-diagnostic-v6"; system_profile="native-startup-mpss-auth" ;;
    *) printf 'Expected preflight, hardware-config, dma-domain, local-memory, buffers, resources, irq, shared, uc, firmware, gsi, command, transfer, tables, receive-memory, receive, rmtfs, mpss-smem/mpss-preauth, mpss-auth or mpss-source-diagnostic\n' >&2; exit 2 ;;
esac
system_profile="${system_profile:-$profile}"
source_diagnostic=false
if [ "$mode" = mpss-source-diagnostic ] || [ "${SUNFISH_SOURCE_GUARD_DIAGNOSTIC:-0}" = 1 ]; then source_diagnostic=true; fi
if [ "${SUNFISH_SOURCE_GUARD_DIAGNOSTIC:-0}" = 1 ] && [ "$mode" != mpss-source-diagnostic ]; then
    printf 'Source diagnostic requires the separate mpss-source-diagnostic output profile\n' >&2
    exit 2
fi
mpss_stage=false
if [ "$mode" = mpss-smem ] || [ "$mode" = mpss-preauth ] || [ "$mode" = mpss-auth ] || [ "$mode" = mpss-source-diagnostic ]; then mpss_stage=true; fi
firmware_stage=false
gsi_stage=false
if [ "$mode" = gsi ] || [ "$mode" = command ] || [ "$mode" = transfer ] || [ "$mode" = tables ] || [ "$mode" = receive-memory ] || [ "$mode" = receive ] || [ "$mode" = rmtfs ] || [ "$mpss_stage" = true ]; then gsi_stage=true; fi
if [ "$mode" = firmware ] || [ "$gsi_stage" = true ]; then firmware_stage=true; fi
checks_mode="$mode"
if [ "$mode" = shared ] || [ "$mode" = uc ] || [ "$firmware_stage" = true ]; then checks_mode=irq; fi
out="${SUNFISH_NATIVE_OUTPUT_DIR:-$root_dir/.cache/sunfish-native-wifi/private/$profile}"
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
if [ "$mode" = mpss-auth ] || [ "$mode" = mpss-source-diagnostic ]; then
    if [ "$source_diagnostic" = true ]; then
        export SUNFISH_SOURCE_GUARD_DIAGNOSTIC=1
        python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out" --checkpoints --native-source-diagnostic > "$out/loader-sdk-build.log" 2>&1
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
if [ "$mode" = shared ] || [ "$mode" = uc ] || [ "$firmware_stage" = true ]; then
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
else
    python3 "$port_dir/audit-native-ipa-linux-source.py" --output "$out/linux-source-audit.json"
fi
if [ "$mode" = rmtfs ] || [ "$mpss_stage" = true ]; then
    python3 "$port_dir/test-native-linux-rmtfs-server.py" > "$out/rmtfs-server-tests.log"
    python3 "$port_dir/audit-native-linux-rmtfs-source.py" --output "$out/rmtfs-source-audit.json"
fi
if [ "$mpss_stage" = true ]; then
    python3 "$port_dir/test-native-mpss-linux-smem.py" > "$out/mpss-smem-tests.log"
    python3 "$port_dir/test-native-mpss-smem-entry.py" > "$out/mpss-smem-entry-tests.log"
    python3 "$port_dir/audit-native-mpss-smem-source.py" --output "$out/mpss-smem-source-audit.json"
fi
if [ "$mode" = mpss-preauth ] || [ "$mode" = mpss-auth ] || [ "$mode" = mpss-source-diagnostic ]; then
    python3 "$port_dir/test-native-mpss-linux-power.py" > "$out/mpss-power-tests.log"
    python3 "$port_dir/test-native-mpss-linux-smp2p.py" > "$out/mpss-smp2p-tests.log"
    python3 "$port_dir/test-native-mpss-preauth-entry.py" > "$out/mpss-preauth-entry-tests.log"
    python3 "$port_dir/audit-native-mpss-preauth-source.py" --output "$out/mpss-preauth-source-audit.json"
fi
if [ "$mode" = mpss-auth ] || [ "$mode" = mpss-source-diagnostic ]; then
    python3 "$port_dir/test-native-mpss-linux-boot.py" > "$out/mpss-boot-tests.log"
    python3 "$port_dir/test-native-mpss-auth-root.py" > "$out/mpss-auth-root-tests.log"
    python3 "$port_dir/audit-native-mpss-auth-source.py" --output "$out/mpss-auth-source-audit.json"
fi
python3 "$port_dir/verify-native-boot-inputs.py" --inputs "$inputs"
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
if [ "$mode" = mpss-auth ] || [ "$mode" = mpss-source-diagnostic ]; then
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
if [ "$mode" = mpss-auth ] || [ "$mode" = mpss-source-diagnostic ]; then
    python3 "$port_dir/validate-native-mpss-auth-boot.py" --output-dir "$out" --reference "$reference"
fi
shasum -a 256 "$out/fastboot.img" > "$out/fastboot.img.sha256"
printf 'Built private native IPA prestart image: %s\n' "$out/fastboot.img"
