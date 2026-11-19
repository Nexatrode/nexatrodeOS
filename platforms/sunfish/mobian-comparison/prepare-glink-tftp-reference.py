#!/usr/bin/env python3
"""Prepare metadata-only GLINK/TFTP reference scripts; performs no device I/O."""
from pathlib import Path
import hashlib, json, os, subprocess

root = Path(__file__).resolve().parents[3]
prior = root / '.cache/sunfish-native-wifi/private/tftp-reference-metadata-capture-v2'
out = root / '.cache/sunfish-native-wifi/private/glink-tftp-reference-prepared-v1'
kernel = root / '.cache/sunfish-mobian-comparison/artifacts/kernel.config'
source_repo = root.parent.parent / '.cache/sunfish-linux-mainline-ref'
commit = '2267e248204e7e051e0f7355298949a6440de579'
def digest(p):
    h=hashlib.sha256()
    with p.open('rb') as f:
        for block in iter(lambda:f.read(1048576),b''): h.update(block)
    return h.hexdigest()
assert digest(kernel) == '17ac093aabdca99e0ffa96d2670d71447f461f8fc5f625d6913829b4f79acef4'
image = root / '.cache/sunfish-mobian-comparison/artifacts/sunfish-linux-comparison-stage0-v2.img'
bundle = root / '.cache/sunfish-mobian-comparison/wifi-preparation/private/cold-wifi-inputs.tar.gz'
assert digest(image) == '6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d'
assert digest(bundle) == '7292f81a6c76d580d774ebfaacad4b8e591ff2125365dffd922f06e1e1b3f2c5'
assert digest(prior/'tftp-metadata-trace.so') == 'adbb2583f3d3e8f026cf91b9fb5add9a945e29862259d666a75fd8b24c516e39'
for option in ('CONFIG_TRACEPOINTS=y','CONFIG_EVENT_TRACING=y','CONFIG_TRACING=y'):
    assert option in kernel.read_text().splitlines(), option
trace_header = subprocess.check_output(['git','-C',str(source_repo),'show',commit+':drivers/rpmsg/qcom_glink_trace.h'])
for event in (b'TRACE_EVENT(qcom_glink_cmd_rx_done,', b'TRACE_EVENT(qcom_glink_cmd_tx_data,'):
    assert event in trace_header
originals = {'start-coldwifi-tftp-metadata.sh':'961295a89726f3566e035bff04b264edb67a34d1a3644d4c393db0917e360fce',
             'continue-after-provider-ready.sh':'ed96d675e8632d647f6f875d8e73ed42737c801166209493b03442aef5354a64'}
for name, expected in originals.items(): assert digest(prior/name) == expected, name
cleanup = r'''    if [ -n "${glink_trace_instance:-}" ] && [ -d "$glink_trace_instance" ]; then
        printf '0\n' > "$glink_trace_instance/tracing_on" || status=1
        cat "$glink_trace_instance/trace" > "$work/logs/glink-events.txt" || status=1
        for stats in "$glink_trace_instance"/per_cpu/cpu*/stats; do
            test -r "$stats" || continue
            printf '\n%s\n' "$stats" >> "$work/logs/glink-event-stats.txt"
            cat "$stats" >> "$work/logs/glink-event-stats.txt"
        done
        rmdir "$glink_trace_instance" || status=1
    fi
'''
setup = r'''# Metadata-only tracepoints: no packet data or filenames. Refuse to start
# the modem if the isolated event instance or expected event fields are absent.
glink_trace_candidate=/sys/kernel/tracing/instances/sunfish_glink_ref
test ! -e "$glink_trace_candidate"
mkdir "$glink_trace_candidate"
glink_trace_instance=$glink_trace_candidate
printf '0\n' > "$glink_trace_instance/tracing_on"
printf '256\n' > "$glink_trace_instance/buffer_size_kb"
for event in qcom_glink_cmd_tx_data qcom_glink_cmd_rx_done; do
    eventdir="$glink_trace_instance/events/qcom_glink/$event"
    test -w "$eventdir/enable"
    for field in iid tx; do grep -Eq "field:.*[[:space:]]${field};" "$eventdir/format"; done
    cp "$eventdir/format" "$work/logs/$event-format.txt"
    printf '1\n' > "$eventdir/enable"
    test "$(cat "$eventdir/enable")" = 1
done
if grep -qw mono "$glink_trace_instance/trace_clock"; then printf 'mono\n' > "$glink_trace_instance/trace_clock"; fi
cat "$glink_trace_instance/trace_clock" > "$work/logs/glink-event-clock.txt"
printf '1\n' > "$glink_trace_instance/tracing_on"
test "$(cat "$glink_trace_instance/tracing_on")" = 1
'''
out.mkdir(mode=0o700,parents=True,exist_ok=False)
for name in originals:
    script=(prior/name).read_text()
    anchor='    journalctl -b -u sunfish-wifi-tftp.service --no-pager -o short-monotonic'
    assert script.count(anchor)==1
    script=script.replace(anchor,cleanup+anchor,1)
    anchor="printf 'MPSS STARTING: %s; one reference-script attempt\\n' \"$rp\""
    assert script.count(anchor)==1
    script=script.replace(anchor,setup+anchor,1)
    path=out/name;path.write_text(script);os.chmod(path,0o700)
    subprocess.run(['sh','-n',str(path)],check=True)
manifest={'reference_boot_sha256':'6ffe9af8bdbfd5ba29711bccab6f80ca5eeda671b47bd80c9b2e160a0be9d40d',
          'input_bundle_sha256':'7292f81a6c76d580d774ebfaacad4b8e591ff2125365dffd922f06e1e1b3f2c5',
          'kernel_config_sha256':digest(kernel),'trace_header_commit':commit,'trace_header_sha256':hashlib.sha256(trace_header).hexdigest(),
          'source_scripts_sha256':originals,'scripts_sha256':{n:digest(out/n) for n in originals},
          'scope':'Temporary Linux RAM-backed reference; TFTP socket metadata plus GLINK data and completion tracepoint metadata. No packet contents or paths intentionally captured by these observers. No network association or flash.',
          'limits':'Tracing changes timing and records all GLINK channels in the private instance. TX tracepoint is before actual transmission. Events include remote/channel names and intent identifiers but no payload. Event loss, concurrency, intent reuse, clock differences and missing timestamps can prevent correlation. Existing bringup logs can contain paths. Tracefs and event formats must be checked at runtime.',
          'device_attempted':False}
(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print('Prepared and shell-checked',out)
