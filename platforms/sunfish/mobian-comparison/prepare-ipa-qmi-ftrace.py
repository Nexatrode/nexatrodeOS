#!/usr/bin/env python3
"""Prepare a scoped, temporary Linux function-entry comparison; no device I/O."""
from pathlib import Path
import hashlib
import json
import subprocess
import tarfile
import tempfile

root = Path(__file__).resolve().parents[3]
base = root / 'platforms/sunfish/mobian-comparison/start-coldwifi.sh'
image = root / '.cache/sunfish-mobian-comparison/artifacts/sunfish-linux-comparison-stage0-v2.img'
bundle = root / '.cache/sunfish-mobian-comparison/wifi-preparation/private/cold-wifi-inputs.tar.gz'
modules = root / '.cache/sunfish-mobian-comparison/artifacts/modules.tar.gz'
config = root / '.cache/sunfish-mobian-comparison/artifacts/kernel.config'
out = root / '.cache/sunfish-native-wifi/private/ipa-qmi-ftrace-prepared-v2'
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
assert digest(base) == '1d97e6f4c5c693271581928703a46ece40ff97f2a00990ed514e199ebb178af6'
assert digest(image) == '6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d'
assert digest(bundle) == '7292f81a6c76d580d774ebfaacad4b8e591ff2125365dffd922f06e1e1b3f2c5'
for option in ('CONFIG_FUNCTION_TRACER=y', 'CONFIG_DYNAMIC_FTRACE=y', 'CONFIG_KALLSYMS_ALL=y'):
    assert option in config.read_text().splitlines(), option

functions = ('ipa_client_new_server', 'ipa_client_init_driver_work', 'ipa_client_init_driver',
             'ipa_server_indication_register', 'ipa_server_driver_init_complete',
             'ipa_qmi_ready', 'ipa_uc_interrupt_handler')
with tarfile.open(modules) as archive:
    module = archive.extractfile('lib/modules/7.1.0-sm7150+/kernel/drivers/net/ipa/ipa.ko').read()
assert hashlib.sha256(module).hexdigest() == 'a7b20182b1c40d2bb01c7fa8b65985b083cf35cd1d430195ebc8908450b04750'
with tempfile.TemporaryDirectory(prefix='sunfish-ipa-ref-module-') as tmp:
    ko = Path(tmp) / 'ipa.ko'
    ko.write_bytes(module)
    symbols = subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-nm', '-an', str(ko)], text=True)
    relocations = subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-readelf', '-r', str(ko)], text=True)
    patchable = relocations.split("Relocation section '.rela__patchable_function_entries'", 1)[1].split('Relocation section ', 1)[0]
    for name in functions:
        matches = [line.split() for line in symbols.splitlines() if line.split()[-1:] == [name]]
        assert len(matches) == 1, name
        address = int(matches[0][0], 16)
        # This kernel's patchable-function entry is two AArch64 instructions
        # before the symbol. Runtime tracefs availability is checked again.
        assert f'.text + {address - 8:x}' in patchable, name

setup = r'''
# A private tracefs instance captures only IPA function entries. This is a
# timing-perturbing kernel tracing experiment; it records no QMI payloads.
trace_root=/sys/kernel/tracing
trace_instance=$trace_root/instances/sunfish_ipa_ref
test -r "$trace_root/available_filter_functions"
test ! -e "$trace_instance"
for function in ipa_client_new_server ipa_client_init_driver_work ipa_client_init_driver ipa_server_indication_register ipa_server_driver_init_complete ipa_qmi_ready ipa_uc_interrupt_handler; do
    awk -v n="$function" '$1 == n {count++; if ($2 == "[ipa]") module++} END {exit !(count == 1 && module == 1)}' "$trace_root/available_filter_functions" || {
        printf 'Required trace function unavailable: %s\n' "$function" > "$work/logs/ipa-ftrace-setup.txt"
        exit 1
    }
done
mkdir "$trace_instance"
capture_ipa_trace() {
    status=$?
    trap - EXIT HUP INT TERM
    set +e
    printf '0\n' > "$trace_instance/tracing_on"
    cat "$trace_instance/trace" > "$work/logs/ipa-function-trace.txt" || status=1
    for stats in "$trace_instance"/per_cpu/cpu*/stats; do
        test -r "$stats" || continue
        printf '\n%s\n' "$stats" >> "$work/logs/ipa-trace-stats.txt"
        cat "$stats" >> "$work/logs/ipa-trace-stats.txt"
    done
    printf 'nop\n' > "$trace_instance/current_tracer"
    rmdir "$trace_instance" || { printf 'Trace instance cleanup failed\n' >> "$work/logs/ipa-ftrace-setup.txt"; status=1; }
    exit "$status"
}
trap capture_ipa_trace EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
printf '0\n' > "$trace_instance/tracing_on"
printf '256\n' > "$trace_instance/buffer_size_kb"
if grep -qw global "$trace_instance/trace_clock"; then printf 'global\n' > "$trace_instance/trace_clock"; fi
cat "$trace_instance/trace_clock" > "$work/logs/ipa-trace-clock.txt"
for function in ipa_client_new_server ipa_client_init_driver_work ipa_client_init_driver ipa_server_indication_register ipa_server_driver_init_complete ipa_qmi_ready ipa_uc_interrupt_handler; do
    printf '%s\n' "$function" >> "$trace_instance/set_ftrace_filter"
done
cat "$trace_instance/set_ftrace_filter" > "$work/logs/ipa-trace-filter.txt"
awk '
  BEGIN { split("ipa_client_new_server ipa_client_init_driver_work ipa_client_init_driver ipa_server_indication_register ipa_server_driver_init_complete ipa_qmi_ready ipa_uc_interrupt_handler", a); for (i in a) wanted[a[i]]=1 }
  { if (!wanted[$1] || seen[$1]++) exit 1; count++ }
  END { if (count != 7) exit 1 }
' "$work/logs/ipa-trace-filter.txt"
printf 'function\n' > "$trace_instance/current_tracer"
printf '1\n' > "$trace_instance/tracing_on"
test "$(cat "$trace_instance/tracing_on")" = 1
printf 'Scoped IPA function-entry tracing active before modem startup\n' > "$work/logs/ipa-ftrace-setup.txt"
'''
text = base.read_text()
anchor = 'test "$(cat "$rp/state")" = offline\n'
assert text.count(anchor) == 1
text = text.replace(anchor, anchor + setup, 1)
out.mkdir(parents=True, mode=0o700, exist_ok=False)
script = out / 'start-coldwifi-ipa-ftrace.sh'
script.write_text(text)
subprocess.run(['sh', '-n', str(script)], check=True)
manifest = {
    'reference_boot_sha256': digest(image), 'input_bundle_sha256': digest(bundle),
    'reference_script_sha256': digest(base), 'ipa_module_sha256': hashlib.sha256(module).hexdigest(),
    'capture_script_sha256': digest(script), 'functions': list(functions),
    'scope': 'One temporary Linux reference boot with existing RAM-backed EFS and firmware. A private tracefs instance records only selected IPA function entries during modem and WLAN firmware startup; no QMI payloads, network association or partition flash.',
    'limitations': 'Tracing changes kernel timing. Function entries do not expose arguments, QMI payloads, return values, successful responses or modem-internal reasons. A missing entry is not proof of no transmission, especially with lost trace events or unavailable symbols. Existing startup uses RAM EFS shadows and verifies underlying devices read-only.',
    'device_actions': [], 'attempt_consumed': False,
}
(out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print('Prepared', script)
print('SHA-256', manifest['capture_script_sha256'])
