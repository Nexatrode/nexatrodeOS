#!/usr/bin/env python3
"""Prepare one Linux locator, GLINK and TFTP comparison without device I/O."""

from pathlib import Path
import hashlib
import json
import subprocess

root = Path(__file__).resolve().parents[3]
private = root / ".cache/sunfish-native-wifi/private"
prior = private / "glink-tftp-reference-prepared-v1"
tracer = private / "tftp-reference-metadata-capture-v2/tftp-metadata-trace.so"
out = private / "elf-locator-reference-prepared-v1"
comparison = root / ".cache/sunfish-mobian-comparison"
boot = comparison / "artifacts/sunfish-linux-comparison-stage0-v2.img"
config = comparison / "artifacts/kernel.config"
bundle = comparison / "wifi-preparation/private/cold-wifi-inputs.tar.gz"
source_repo = root.parent.parent / ".cache/sunfish-linux-mainline-ref"
commit = "2267e248204e7e051e0f7355298949a6440de579"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def pinned_source(path, expected):
    data = subprocess.check_output(["git", "-C", str(source_repo), "show", f"{commit}:{path}"])
    assert hashlib.sha256(data).hexdigest() == expected, path
    return data


def replace_once(script, anchor, replacement):
    assert script.count(anchor) == 1, anchor
    return script.replace(anchor, replacement, 1)


inputs = {
    boot: "6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d",
    bundle: "7292f81a6c76d580d774ebfaacad4b8e591ff2125365dffd922f06e1e1b3f2c5",
    config: "17ac093aabdca99e0ffa96d2670d71447f461f8fc5f625d6913829b4f79acef4",
    tracer: "adbb2583f3d3e8f026cf91b9fb5add9a945e29862259d666a75fd8b24c516e39",
    prior / "start-coldwifi-tftp-metadata.sh": "5eeda72436e98deb6f46b9dd64f73c6c7cf5407ccae03b174921fd189015dae4",
    prior / "continue-after-provider-ready.sh": "675fb96ec0174c7163add4b2df5af253a1dcb7524ef56aef3d239626eb324b26",
}
for path, expected in inputs.items():
    assert digest(path) == expected, path

for option in ("CONFIG_DYNAMIC_DEBUG=y", "CONFIG_QCOM_PD_MAPPER=m",
               "CONFIG_PRINTK_TIME=y", "CONFIG_EVENT_TRACING=y", "CONFIG_QRTR=y"):
    assert option in config.read_text().splitlines(), option

source_hashes = {
    "drivers/soc/qcom/qcom_pd_mapper.c": "3372fd32381a606d530bc7d624fe4a037954d519b4ed3c970547c0d961573010",
    "include/trace/events/qrtr.h": "47027170b526ce1f89837aca745513cebee549618673348dfcb5415ea44cbabd",
}
mapper, qrtr = (pinned_source(path, sha) for path, sha in source_hashes.items())
lookup = mapper.split(b"static void qcom_pdm_get_domain_list(", 1)[1].split(b"static void qcom_pdm_pfr(", 1)[0]
assert lookup.count(b"pr_debug(") == 2
assert b'pr_debug("PDM: found ' in lookup and b'pr_debug("PDM: service ' in lookup
assert b"TRACE_EVENT(qrtr_ns_server_add," in qrtr

# The saved reference uses the same trace clock as the TFTP observer.
trace_setup = r'''eventdir="$glink_trace_instance/events/qrtr/qrtr_ns_server_add"
test -w "$eventdir/enable"
for field in service instance node port; do
    grep -Eq "field:.*[[:space:]]${field};" "$eventdir/format"
done
cp "$eventdir/format" "$work/logs/qrtr_ns_server_add-format.txt"
printf '1\n' > "$eventdir/enable"
test "$(cat "$eventdir/enable")" = 1
grep -qw mono "$glink_trace_instance/trace_clock"
printf 'mono\n' > "$glink_trace_instance/trace_clock"
grep -Fq '[mono]' "$glink_trace_instance/trace_clock"'''

locator_setup = r'''# Require the two known mapper sites before issuing the single modem start.
modprobe qcom_pd_mapper
locator_control=/proc/dynamic_debug/control
if [ ! -w "$locator_control" ]; then
    locator_control=/sys/kernel/debug/dynamic_debug/control
fi
test -w "$locator_control"
locator_sites='\[qcom_pd_mapper\]qcom_pdm_get_domain_list[[:space:]]'
grep -E "$locator_sites" "$locator_control" > "$work/logs/locator-debug-before.txt"
awk 'NF {n++; if ($3 ~ /p/) bad=1} END {exit !(n == 2 && !bad)}' "$work/logs/locator-debug-before.txt"
restore_locator_debug=1
printf 'module qcom_pd_mapper func qcom_pdm_get_domain_list +p\n' > "$locator_control"
grep -E "$locator_sites" "$locator_control" > "$work/logs/locator-debug-enabled.txt"
awk 'NF {n++; if ($3 !~ /p/) bad=1} END {exit !(n == 2 && !bad)}' "$work/logs/locator-debug-enabled.txt"
'''

locator_cleanup = r'''    if [ "${restore_locator_debug:-0}" = 1 ]; then
        printf 'module qcom_pd_mapper func qcom_pdm_get_domain_list -p\n' > "$locator_control" || status=1
        grep -E "$locator_sites" "$locator_control" > "$work/logs/locator-debug-after.txt" || status=1
        awk 'NF {n++; if ($3 ~ /p/) bad=1} END {exit !(n == 2 && !bad)}' "$work/logs/locator-debug-after.txt" || status=1
        dmesg > "$work/locator-kmsg.tmp" || status=1
        awk '/PDM: (found|service) /' "$work/locator-kmsg.tmp" > "$work/logs/locator-events.txt" || status=1
        rm -f "$work/locator-kmsg.tmp" || status=1
    fi
'''

tftp_old = '''    journalctl -b -u sunfish-wifi-tftp.service --no-pager -o short-monotonic | awk 'index($0, "TFTP_META event=")' > "$work/logs/tftp-metadata.txt"'''
tftp_capture = r'''    journalctl -b -u sunfish-wifi-tftp.service --no-pager -o short-monotonic > "$work/tftp-journal.tmp" || status=1
    awk 'index($0, "TFTP_META event=")' "$work/tftp-journal.tmp" > "$work/logs/tftp-metadata.txt" || status=1
    awk '/\[TQFTP\] (RRQ|WRQ): \//' "$work/tftp-journal.tmp" > "$work/logs/tftp-paths.txt" || status=1
    rm -f "$work/tftp-journal.tmp" || status=1'''
modem_start = "printf 'MPSS STARTING: %s; one reference-script attempt\\n' \"$rp\""
old_clock = '''if grep -qw mono "$glink_trace_instance/trace_clock"; then printf 'mono\\n' > "$glink_trace_instance/trace_clock"; fi'''

out.mkdir(mode=0o700)
scripts = {}
for name in ("start-coldwifi-tftp-metadata.sh", "continue-after-provider-ready.sh"):
    original = (prior / name).read_text()
    script = replace_once(original, old_clock, trace_setup)
    script = replace_once(script, modem_start, locator_setup + modem_start)
    script = replace_once(script, tftp_old, locator_cleanup + tftp_capture)
    path = out / name
    path.write_text(script)
    path.chmod(0o700)
    subprocess.run(["sh", "-n", str(path)], check=True)
    scripts[name] = digest(path)

manifest = {
    "product": "sunfish",
    "reference_boot_sha256": inputs[boot],
    "input_bundle_sha256": inputs[bundle],
    "kernel_commit": commit,
    "source_sha256": source_hashes,
    "input_sha256": {str(path.relative_to(root)): sha for path, sha in inputs.items()},
    "script_sha256": scripts,
    "scope": "One temporary Linux boot, RAM-backed modem storage and reference WLAN firmware startup without network association. Collect only locator lookup names and counts, GLINK/QRTR tracepoint metadata and TFTP paths and metadata in the comparison logs.",
    "limits": "Observers can affect timing. The locator debug sites do not include QMI transaction IDs. Trace loss, clock offsets and concurrent traffic can limit correlation. Observer activation and cleanup are checked on the reference kernel at runtime.",
    "attempt_consumed": False,
}
(out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(out / "manifest.json")
