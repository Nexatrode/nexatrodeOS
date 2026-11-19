#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="${SUNFISH_MODEM_IMAGE_OUT_DIR:-$root_dir/.cache/pixel-sunfish/modem-image-probe}"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
image_bin="${SUNFISH_MODEM_IMAGE_BIN:-rust_sunfish_modem_image_probe}"
case "$image_bin" in rust_sunfish_modem_image_probe|rust_sunfish_modem_image_boot_check|rust_sunfish_modem_auth_probe|rust_sunfish_modem_ready_probe|rust_sunfish_modem_smp2p_probe) ;; *) exit 1 ;; esac
case "${SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC:-0}${SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE:-0}${SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM:-0}${SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY:-0}${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" in
    00000|10000|01000|00100|00010|00001) ;; *) exit 1 ;;
esac
system="$port_dir/modem-image-probe.system"
if [ "$image_bin" = rust_sunfish_modem_smp2p_probe ]; then
    test "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1
fi
if [ "$image_bin" = rust_sunfish_modem_ready_probe ]; then
    test "${SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY:-0}" = 1
fi
if [ "$image_bin" = rust_sunfish_modem_auth_probe ]; then
    test "${SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM:-0}" = 1
fi
case "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION:-0}${SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION:-0}${SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY:-0}" in
    000) ;;
    100|010|001)
    if [ "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1 ]; then
        test "$image_bin" = rust_sunfish_modem_smp2p_probe
    elif [ "${SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY:-0}" = 1 ]; then
        test "$image_bin" = rust_sunfish_modem_ready_probe
    elif [ "${SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM:-0}" = 1 ]; then
        test "$image_bin" = rust_sunfish_modem_auth_probe
    elif [ "${SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE:-0}" = 1 ]; then
        test "$image_bin" = rust_sunfish_modem_image_probe
    else
        test "$image_bin" = rust_sunfish_modem_image_boot_check
    fi
    test "${SUNFISH_IMAGE_LOADER_CHECKPOINTS:-0}" = 1
    ;;
    *) exit 1 ;;
esac
if [ "${SUNFISH_IMAGE_LOADER_COPY_PROGRESS:-0}" = 1 ]; then
    test "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY:-0}" = 1
fi
if [ "${SUNFISH_IMAGE_LOADER_COPY_INPUTS:-0}" = 1 ]; then
    test "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_COPY_PROGRESS:-0}" = 0
fi
if [ "${SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS:-0}" = 1 ]; then
    test "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION:-0}" = 1
fi
if [ "${SUNFISH_IMAGE_LOADER_TASK_IMAGES:-0}" = 1 ]; then
    test "${SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS:-0}" = 1
fi
if [ "${SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC:-0}" = 1 ]; then
    test "${SUNFISH_IMAGE_LOADER_TASK_IMAGES:-0}" = 1
    test "$image_bin" = rust_sunfish_modem_image_boot_check
    test "${SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE:-0}" = 0
fi
if [ "${SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE:-0}" = 1 ]; then
    test "${SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC:-0}" = 0
    test "${SUNFISH_IMAGE_LOADER_TASK_IMAGES:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_CHECKPOINTS:-0}" = 1
    test "$image_bin" = rust_sunfish_modem_image_probe
fi
if [ "${SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM:-0}" = 1 ]; then
    test "$image_bin" = rust_sunfish_modem_auth_probe
    test "${SUNFISH_IMAGE_LOADER_TASK_IMAGES:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_CHECKPOINTS:-0}" = 1
fi
if [ "${SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY:-0}" = 1 ]; then
    test "$image_bin" = rust_sunfish_modem_ready_probe
    test "${SUNFISH_IMAGE_LOADER_TASK_IMAGES:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_CHECKPOINTS:-0}" = 1
    system="$port_dir/modem-ready-probe.system"
fi
if [ "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1 ]; then
    test "$image_bin" = rust_sunfish_modem_smp2p_probe
    test "${SUNFISH_IMAGE_LOADER_TASK_IMAGES:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION:-0}" = 1
    test "${SUNFISH_IMAGE_LOADER_CHECKPOINTS:-0}" = 1
    system="$port_dir/modem-smp2p-probe.system"
fi
case "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" in
    0) ;;
    1)
        test "${SUNFISH_MODEM_CRASH_PROBE:-0}" = 1
        test "${SUNFISH_QRTR_WAIT_PROBE:-0}" = 1
        test "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1
        test "$image_bin" = rust_sunfish_modem_smp2p_probe
        test "${SUNFISH_GLINK_RECEIVE_PROBE:-0}" = 0
        test "${SUNFISH_GLINK_PREPARE_PROBE:-0}" = 0
        test "${SUNFISH_GLINK_BOOT_PROBE:-0}" = 0
        ;;
    *) exit 1 ;;
esac
export SUNFISH_MODEM_IMAGE_BIN="$image_bin"
image_features=sunfish-firmware-metadata-probe
case "${SUNFISH_GLINK_PREPARE_PROBE:-0}${SUNFISH_GLINK_RECEIVE_PROBE:-0}${SUNFISH_GLINK_BOOT_PROBE:-0}${SUNFISH_GLINK_VERSION_PROBE:-0}" in 0000|1000|0100|0010|0001) ;; *) exit 1 ;; esac
case "${SUNFISH_GLINK_PENDING_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_GLINK_VERSION_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_GLINK_OPEN_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_GLINK_PENDING_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_GLINK_OPEN_PLAN_SNAPSHOT:-0}" in
    0) ;;
    1)
        test "${SUNFISH_GLINK_OPEN_PROBE:-0}" = 1
        test "${SUNFISH_GLINK_CHANNEL_PROBE:-0}" = 0
        test "${SUNFISH_GLINK_INTENTS_PROBE:-0}" = 0
        test "${SUNFISH_GLINK_RX_PACKET_PROBE:-0}" = 0
        test "${SUNFISH_GLINK_DONE_PROBE:-0}" = 0
        test "${SUNFISH_QRTR_HELLO_PROBE:-0}" = 0
        test "${SUNFISH_QRTR_NEXT_PROBE:-0}" = 0
        test "${SUNFISH_QRTR_DISCOVERY_PROBE:-0}" = 0
        test "${SUNFISH_QRTR_CREDIT_PROBE:-0}" = 0
        test "${SUNFISH_QRTR_WAIT_PROBE:-0}" = 0
        test "${SUNFISH_QRTR_SERVREG_PROBE:-0}" = 0
        ;;
    *) exit 1 ;;
esac
case "${SUNFISH_GLINK_CHANNEL_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_GLINK_OPEN_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_GLINK_INTENTS_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_GLINK_CHANNEL_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_GLINK_RX_PACKET_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_GLINK_INTENTS_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_GLINK_DONE_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_GLINK_RX_PACKET_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_QRTR_HELLO_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_GLINK_DONE_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_QRTR_NEXT_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_QRTR_HELLO_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_QRTR_DISCOVERY_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_QRTR_NEXT_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_QRTR_CREDIT_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_QRTR_DISCOVERY_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_QRTR_WAIT_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_QRTR_CREDIT_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_MODEM_CRASH_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_QRTR_WAIT_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" in 0) ;; 1) test "${SUNFISH_MODEM_CRASH_PROBE:-0}" = 1 ;; *) exit 1 ;; esac
case "${SUNFISH_GLINK_RECEIVE_PROBE:-0}" in
    0) ;;
    1) test "$image_bin" = rust_sunfish_modem_smp2p_probe
       test "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1
       image_features="$image_features,sunfish-glink-receive-probe" ;;
    *) exit 1 ;;
esac
if [ "${SUNFISH_GLINK_PREPARE_PROBE:-0}" = 1 ]; then
    test "$image_bin" = rust_sunfish_modem_smp2p_probe
    test "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1
    image_features="$image_features,sunfish-glink-prepare-probe"
    system="$port_dir/modem-glink-prepare-probe.system"
fi
if [ "${SUNFISH_GLINK_BOOT_PROBE:-0}" = 1 ]; then
    test "$image_bin" = rust_sunfish_modem_smp2p_probe
    test "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1
    image_features="$image_features,sunfish-glink-boot-probe"
    system="$port_dir/modem-glink-prepare-probe.system"
fi
if [ "${SUNFISH_GLINK_VERSION_PROBE:-0}" = 1 ]; then
    test "$image_bin" = rust_sunfish_modem_smp2p_probe
    test "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1
    image_features="$image_features,sunfish-glink-version-probe"
    system="$port_dir/modem-glink-prepare-probe.system"
fi
if [ "${SUNFISH_GLINK_PENDING_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-glink-pending-probe"
fi
if [ "${SUNFISH_GLINK_OPEN_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-glink-open-probe"
fi
if [ "${SUNFISH_GLINK_OPEN_PLAN_SNAPSHOT:-0}" = 1 ]; then
    image_features="$image_features,sunfish-glink-open-plan-snapshot"
fi
if [ "${SUNFISH_GLINK_CHANNEL_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-glink-channel-probe"
fi
if [ "${SUNFISH_GLINK_INTENTS_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-glink-intents-probe"
fi
if [ "${SUNFISH_GLINK_RX_PACKET_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-glink-rx-packet-probe"
fi
if [ "${SUNFISH_GLINK_DONE_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-glink-done-probe"
fi
if [ "${SUNFISH_QRTR_HELLO_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-qrtr-hello-probe"
fi
if [ "${SUNFISH_QRTR_NEXT_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-qrtr-next-probe"
fi
if [ "${SUNFISH_QRTR_DISCOVERY_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-qrtr-discovery-probe"
fi
if [ "${SUNFISH_QRTR_CREDIT_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-qrtr-credit-probe"
fi
if [ "${SUNFISH_QRTR_WAIT_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-qrtr-wait-probe"
fi
if [ "${SUNFISH_QRTR_SERVREG_PROBE:-0}" = 1 ]; then
    test "$image_bin" = rust_sunfish_modem_smp2p_probe
    test "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1
    image_features="$image_features,sunfish-qrtr-servreg-probe"
    system="$port_dir/modem-glink-prepare-probe.system"
fi
if [ "${SUNFISH_MODEM_SERVREG_STATE_PROBE:-0}" = 1 ]; then
    test "$image_bin" = rust_sunfish_modem_smp2p_probe
    test "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1
    test "${SUNFISH_QRTR_SERVREG_PROBE:-0}" = 0
    test "${SUNFISH_MODEM_CRASH_PROBE:-0}" = 0
    test "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" = 0
    test "${SUNFISH_GLINK_PREPARE_PROBE:-0}${SUNFISH_GLINK_RECEIVE_PROBE:-0}${SUNFISH_GLINK_BOOT_PROBE:-0}${SUNFISH_GLINK_VERSION_PROBE:-0}" = 0000
    test "${SUNFISH_GLINK_OPEN_PROBE:-0}${SUNFISH_GLINK_CHANNEL_PROBE:-0}${SUNFISH_GLINK_INTENTS_PROBE:-0}${SUNFISH_GLINK_RX_PACKET_PROBE:-0}${SUNFISH_GLINK_DONE_PROBE:-0}" = 00000
    test "${SUNFISH_QRTR_HELLO_PROBE:-0}${SUNFISH_QRTR_NEXT_PROBE:-0}${SUNFISH_QRTR_DISCOVERY_PROBE:-0}${SUNFISH_QRTR_CREDIT_PROBE:-0}${SUNFISH_QRTR_WAIT_PROBE:-0}" = 00000
    image_features="$image_features,sunfish-modem-servreg-state-probe"
    system="$port_dir/modem-servreg-state-probe.system"
fi
if [ "${SUNFISH_MODEM_SERVREG_RESTART_PROBE:-0}" = 1 ]; then
    test "${SUNFISH_MODEM_SERVREG_STATE_PROBE:-0}" = 1
    image_features="$image_features,sunfish-modem-servreg-restart-probe"
fi
if [ "${SUNFISH_MODEM_TFTP_OBSERVER:-0}" = 1 ]; then
    test "${SUNFISH_MODEM_SERVREG_STATE_PROBE:-0}" = 1
    test "${SUNFISH_MODEM_SERVREG_RESTART_PROBE:-0}" = 0
    image_features="$image_features,sunfish-modem-tftp-observer"
fi
if [ "${SUNFISH_QRTR_SERVREG_PREQUERY_CHECK:-0}" = 1 ]; then
    test "${SUNFISH_QRTR_SERVREG_PROBE:-0}" = 1
    image_features="$image_features,sunfish-qrtr-servreg-prequery-check"
fi
if [ "${SUNFISH_QRTR_SERVREG_QUERY_CHECK:-0}" = 1 ]; then
    test "${SUNFISH_QRTR_SERVREG_PROBE:-0}" = 1
    test "${SUNFISH_QRTR_SERVREG_PREQUERY_CHECK:-0}" = 0
    image_features="$image_features,sunfish-qrtr-servreg-query-check"
fi
if [ "${SUNFISH_QRTR_SERVREG_PDR_REPLY:-0}" = 1 ]; then
    test "${SUNFISH_QRTR_SERVREG_PROBE:-0}" = 1
    test "${SUNFISH_QRTR_SERVREG_PREQUERY_CHECK:-0}" = 0
    test "${SUNFISH_QRTR_SERVREG_QUERY_CHECK:-0}" = 0
    image_features="$image_features,sunfish-qrtr-servreg-pdr-reply"
fi
if [ "${SUNFISH_QRTR_SERVREG_PDR_FOLLOWUP:-0}" = 1 ]; then
    test "${SUNFISH_QRTR_SERVREG_PDR_REPLY:-0}" = 1
    image_features="$image_features,sunfish-qrtr-servreg-pdr-followup"
fi
if [ "${SUNFISH_MODEM_CRASH_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-modem-crash-probe"
fi
if [ "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" = 1 ]; then
    image_features="$image_features,sunfish-modem-fatal-trace-probe"
    system="$port_dir/modem-fatal-trace-probe.system"
fi
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
export SUNFISH_MODEM_MDT_PATH="$root_dir/.cache/pixel-sunfish/pas-metadata-prepare/modem.mdt"
export SUNFISH_MODEM_IMAGE_DIR="$root_dir/.cache/pixel-sunfish/modem-image-prepare"
export SUNFISH_MODEM_SOURCES_RS_PATH="$out_dir/firmware-sources.rs"
export SUNFISH_MODEM_IMAGE_PROFILE="$out_dir/metadata-profile.json"
test -f "$reference"
test -f "$SUNFISH_MODEM_MDT_PATH"
test -d "$SUNFISH_MODEM_IMAGE_DIR"
test -x "$sdk/bin/microkit"
mkdir -p "$out_dir"
cd "$root_dir"
python3 - <<'PY'
import hashlib, json, os, struct
from pathlib import Path
record=json.loads(Path('platforms/sunfish/wlan-resources.json').read_text())['modem_firmware_observation']['mdt']
b=Path(os.environ['SUNFISH_MODEM_MDT_PATH']).read_bytes()
assert len(b)==record['size_bytes']==9100
assert hashlib.sha256(b).hexdigest()==record['sha256']=='43a1414628153c109f06fe70385946c1c530a4ee08840b40f62649c9fa589870'
assert b[:7]==b'\x7fELF\x01\x01\x01' and struct.unpack_from('<H',b,18)[0]==164
phoff=struct.unpack_from('<I',b,28)[0]
count=struct.unpack_from('<H',b,44)[0]
assert count==31 and struct.unpack_from('<H',b,42)[0]==32
assert phoff+count*32 <= len(b)
segments=[]
for i in range(count):
    p=struct.unpack_from('<8I',b,phoff+i*32)
    if p[0]==1 and p[6] & (7<<24) != 2<<24 and p[5]:
        assert p[6] & (1<<27) and p[4]<=p[5]
        segments.append((p[3],p[3]+p[5]))
assert len(segments)==29
assert min(s[0] for s in segments)==0x8b000000
assert max((s[1]+4095)&~4095 for s in segments)==0x94800000
for i,a in enumerate(segments):
    assert all(not(a[0]<b[1] and b[0]<a[1]) for b in segments[i+1:])
inventory={r['name']:r for r in json.loads(Path('platforms/sunfish/wlan-resources.json').read_text())['modem_firmware_observation']['segment_hash_inventory']}
byte_sum=0
files=[];generated=["static FIRMWARE_SOURCES: [Source<'static>;29] = ["]
for i in range(count):
    p=struct.unpack_from('<8I',b,phoff+i*32)
    if not(p[0]==1 and p[6] & (7<<24) != 2<<24 and p[5]): continue
    name='modem.b%02d'%i;path=Path(os.environ['SUNFISH_MODEM_IMAGE_DIR'])/name;data=path.read_bytes();record=inventory[name]
    assert len(data)==record['size_bytes']==p[4] and hashlib.sha256(data).hexdigest()==record['sha256']
    byte_sum=(byte_sum+sum(data)) & 0xffffffff
    generated.append('    Source { index: %d, bytes: include_bytes!(%s) },'%(i,json.dumps(str(path))))
    files.append(dict(index=i,physical=hex(p[3]),file_bytes=p[4],memory_bytes=p[5],flags=hex(p[6]),sha256=record['sha256']))
generated.append('];');generated.append('const FIRMWARE_BYTE_SUM:u32 = %d;'%byte_sum);Path(os.environ['SUNFISH_MODEM_SOURCES_RS_PATH']).write_text('\n'.join(generated)+'\n')
assert len(files)==29 and sum(f['file_bytes'] for f in files)==68393962 and sum(f['memory_bytes'] for f in files)==157744117
assert 0x9800000-sum(f['memory_bytes'] for f in files)==1639435
profile={
    'probe_bin':os.environ['SUNFISH_MODEM_IMAGE_BIN'],'diagnostic_byte_sum':byte_sum,
    'glink_receive_header_probe':os.environ.get('SUNFISH_GLINK_RECEIVE_PROBE')=='1',
    'glink_prepare_memory_probe':os.environ.get('SUNFISH_GLINK_PREPARE_PROBE')=='1',
    'glink_boot_probe':os.environ.get('SUNFISH_GLINK_BOOT_PROBE')=='1',
    'glink_version_probe':os.environ.get('SUNFISH_GLINK_VERSION_PROBE')=='1',
    'glink_pending_probe':os.environ.get('SUNFISH_GLINK_PENDING_PROBE')=='1',
    'glink_open_probe':os.environ.get('SUNFISH_GLINK_OPEN_PROBE')=='1',
    'glink_channel_probe':os.environ.get('SUNFISH_GLINK_CHANNEL_PROBE')=='1',
    'glink_intents_probe':os.environ.get('SUNFISH_GLINK_INTENTS_PROBE')=='1',
    'glink_rx_packet_probe':os.environ.get('SUNFISH_GLINK_RX_PACKET_PROBE')=='1',
    'glink_done_probe':os.environ.get('SUNFISH_GLINK_DONE_PROBE')=='1',
    'qrtr_hello_probe':os.environ.get('SUNFISH_QRTR_HELLO_PROBE')=='1',
    'modem_crash_probe':os.environ.get('SUNFISH_MODEM_CRASH_PROBE')=='1',
    'modem_fatal_trace_probe':os.environ.get('SUNFISH_MODEM_FATAL_TRACE_PROBE')=='1',
    'modem_fatal_trace_scope':'Repeat the existing measured-wait test with no added QRTR, GLINK, SMEM, doorbell, SMC, DMA or partition-write authority. Record sampled slave-kernel status transitions and the bounded item421 reason snapshot; retain named SMP2P fatal/ready/proxy/stop/shutdown bits. Do not map the GIC distributor: the seL4 kernel platform map excludes its distributor page from user device untypeds. No IRQ capability, GIC read/write, or acknowledgement is added.' if os.environ.get('SUNFISH_MODEM_FATAL_TRACE_PROBE')=='1' else None,
    'modem_crash_scope':'Repeat the measured-wait transport policy unchanged. Capture bounded read-only global SMEM item421 before any secure call and after the terminal discovery result. Validate global v12 host pair65534/65534, partition nonoverlap, all allocation bounds, duplicate item rejection, two equal bounded payload copies and matching metadata; at most3 attempts on instability. Sample raw SMP2P status read-only after terminal failure without requiring readiness. Compare captured256byte prefixes to identify unchanged baseline content; sanitized text is diagnostic data only. No reason clear, arbitrary address, additional packet/index/doorbell writes, QMI, DMA, cleanup, reset, buffer release or retry.' if os.environ.get('SUNFISH_MODEM_CRASH_PROBE')=='1' else None,
    'qrtr_wait_probe':os.environ.get('SUNFISH_QRTR_WAIT_PROBE')=='1',
    'qrtr_servreg_probe':os.environ.get('SUNFISH_QRTR_SERVREG_PROBE')=='1',
    'qrtr_servreg_probe':os.environ.get('SUNFISH_QRTR_SERVREG_PROBE')=='1',
    'qrtr_servreg_prequery_check':os.environ.get('SUNFISH_QRTR_SERVREG_PREQUERY_CHECK')=='1',
    'qrtr_servreg_query_check':os.environ.get('SUNFISH_QRTR_SERVREG_QUERY_CHECK')=='1',
    'qrtr_servreg_pdr_reply':os.environ.get('SUNFISH_QRTR_SERVREG_PDR_REPLY')=='1',
    'qrtr_servreg_pdr_followup':os.environ.get('SUNFISH_QRTR_SERVREG_PDR_FOLLOWUP')=='1',
    'modem_servreg_state_probe':os.environ.get('SUNFISH_MODEM_SERVREG_STATE_PROBE')=='1',
    'modem_servreg_restart_probe':os.environ.get('SUNFISH_MODEM_SERVREG_RESTART_PROBE')=='1',
    'modem_tftp_observer':os.environ.get('SUNFISH_MODEM_TFTP_OBSERVER')=='1',
    'modem_tftp_observer_scope':'Advertise one local QRTR TFTP endpoint beside the one read-only SERVREG state query; capture only bounded, sanitized RRQ/WRQ metadata (peer, opcode, path, mode and options), never file bytes. Send no TFTP response or firmware, perform no PDR restart, MSA handoff, WLAN DMA, or radio start.' if os.environ.get('SUNFISH_MODEM_TFTP_OBSERVER')=='1' else None,
    'modem_servreg_state_probe_scope':'After the required modem PAS initialize/authenticated boot path, use the validated owner/FIFO path to bind one SERVREG_NOTIF 0x42/version1 server and query only the resolved wlan/fw domain path msm/modem/wlan_pd. Accept only its transaction/endpoint-matched response and preserve RX confirmation. Do not register/start WLFW, request WLAN MSA_INFO/MSA_READY, grant WLAN MSA memory to secure world, set up WLAN DMA, or issue another query.' if os.environ.get('SUNFISH_MODEM_SERVREG_STATE_PROBE')=='1' and os.environ.get('SUNFISH_MODEM_SERVREG_RESTART_PROBE')!='1' else None,
    'modem_servreg_restart_probe_scope':'After the required modem PAS initialize/authenticated boot path, use the validated owner/FIFO path to bind the exact SERVREG_NOTIF 0x42/version1 instance B401. REGISTER_LISTENER for the resolved WLAN protection-domain path msm/modem/wlan_pd, not the wlan/fw Service Locator alias. Accept only its endpoint/transaction-matched success response; use the returned current state and send at most one transaction-bound RESTART_PD only when that state is UNINIT. Also publish the bounded manifest-backed SERVREG domain-map endpoint on a distinct local QRTR port and bind the exact WLFW 0x45/version1/instance0 server. Send only the one source-defined WLFW indication-registration request and accept its exact endpoint/transaction-matched response. Send one bounded MSA_INFO request only after the successful RESTART_PD response and successful WLFW registration, then decode the exact response without initiating SCM ownership transfer. On any failure, changed state, or failed live lifecycle check, send no restart and never retry. No QUERY_STATE, MSA_READY, storage/heap endpoint, WLAN MSA grant, WLAN DMA, firmware transfer, radio start, or partition write.' if os.environ.get('SUNFISH_MODEM_SERVREG_RESTART_PROBE')=='1' else None,
    'qrtr_servreg_prequery_check_scope':'Repeat the fixed SERVREG announcement after the prerequisite QRTR/GLINK setup, then inspect the bounded incoming snapshot with stable double reads. For the exact previously captured 80-byte shape, display the candidate GLINK/QRTR/QMI field decode only and preserve that snapshot RX tail; send no QMI response. If the alternate observed 88-byte prelude appears, validate and report it, then stop before consuming that prelude, publishing its follow-up receive intent, or sending a SERVREG QMI response. No additional service lookup, WLAN QMI, DMA, retry, intent reuse or cleanup.' if os.environ.get('SUNFISH_QRTR_SERVREG_PREQUERY_CHECK')=='1' else None,
    'qrtr_servreg_query_check_scope':'Repeat the fixed SERVREG announcement after the prerequisite QRTR/GLINK setup. Accept only the previously validated 88-byte prelude, consume it once, and publish the single fixed follow-up receive intent. Double-copy and validate an exact tms/servreg GET_DOMAIN_LIST request before reporting it; stop before consuming that query or sending a QMI response. For either exact observed 80-byte message-0x21 candidate (tms/servreg or tms/pdr_enabled), show the bounded field decode, preserve its RX tail, and send no response. Reject all other packet shapes without advancing their RX tail. No extra service lookup, WLAN QMI, DMA, retry, intent reuse or cleanup.' if os.environ.get('SUNFISH_QRTR_SERVREG_QUERY_CHECK')=='1' else None,
    'qrtr_servreg_pdr_reply_scope':'Repeat the validated SERVREG endpoint/prelude setup, then accept only the exact observed 80-byte GLINK receive with chunk60 and QMI GET_DOMAIN_LIST for tms/pdr_enabled. Double-copy and validate the complete 57-byte QRTR/QMI datagram and zero padding. Reply once with QMI success, database revision1 and total_domains0, omitting the optional domain-list TLV because neither exact Sunfish manifest advertises this service. Preserve the request transaction and endpoint, consume only that exact request, verify GLINK consumes the one reply, then passively observe RX for at most2s/256 polls using stable double reads and capture at most64 bytes without advancing RX tail. No second reply, PFR, other SERVREG query, additional lookup, WLAN QMI, DMA, retry, intent reuse or cleanup.' if os.environ.get('SUNFISH_QRTR_SERVREG_PDR_REPLY')=='1' else None,
    'qrtr_servreg_pdr_followup_scope':'Repeat the fixed SERVREG setup and exact tms/pdr_enabled empty-map reply. After its consumption, accept only RX_DONE_W_REUSE14/channel2/remote intent2 plus RX_INTENT_REQ7/channel2/size64; consume exactly16 bytes and advertise fresh local intent5/64 bytes with positive ACK. Double-copy at most96 RX bytes without moving the data tail, then accept only the captured80-byte GLINK DATA9/channel2/intent5 frame containing the exact 61-byte QRTR/QMI GET_DOMAIN_LIST request for tms/pddump_disabled and three zero pad bytes. Consume exactly that80-byte request once, send one empty-map QMI success preserving transaction and endpoint, and verify consumption. Then double-copy the exact post-reply RX_DONE_W_REUSE14/channel2/intent2 record, consume only those8 bytes, and passively observe for at most10s/1200 polls, capturing at most64 subsequent RX bytes while holding the new RX tail fixed. No third QMI response, PFR, other lookup, WLAN QMI, DMA, retry, intent reuse or cleanup.' if os.environ.get('SUNFISH_QRTR_SERVREG_PDR_FOLLOWUP')=='1' else None,
    'qrtr_servreg_scope':'Publish one local QRTR SERVREG service 0x40 endpoint at node1/port0x4000 using distinct pre-advertised peer intents. Accept only the observed 88-byte prelude: QRTR NEW_SERVER for service0x2b/instance0x1202/port1, GLINK RX_DONE_W_REUSE intent1, and RX_INTENT_REQ size60. Validate the whole prelude before retiring local intent3, consuming those 88 bytes, and advertising fresh local intent4 with a 64-byte buffer. Then accept only a 72-byte GLINK packet containing QMI GET_DOMAIN_LIST (message0x21) for tms/servreg and answer once with the captured root_pd and wlan_pd instance180 mappings, including zero service_data_valid/service_data fields. On either exact observed 80-byte message-0x21 candidate (tms/servreg or tms/pdr_enabled), display a bounded GLINK/QRTR/QMI field decode only; preserve the RX tail and send no QMI response. Require exact wire lengths, transaction preservation, two stable FIFO reads, private buffer guards and stable queue/identity checks. Stop after response consumption or the first malformed/unexpected state. No PFR, additional service lookup, WLAN QMI, DMA, retry, intent reuse or cleanup.' if os.environ.get('SUNFISH_QRTR_SERVREG_PROBE')=='1' else None,
    'qrtr_wait_scope':'Measured passive DATA wait using the exact optional-request fresh-credit policy and packet/write ceilings. Permit up to60seconds per pending DATA,8192 data samples, and65seconds globally in the discovery phase, with independent stopped-counter work bounds. Record last-poll elapsed milliseconds, counter frequency, DATA polls and last successfully observed slave-kernel status. No additional control messages for a given packet sequence, no reoffer on silence or receive consumption on timeout. Retain all buffers/leases and stop immediately on malformed input or lost readiness. Up to32 new packets/33 records,TX1192/RX3312,64 additional notifications,1536 additional SMEM writes maxima; no service QMI, extra QRTR beyond HELLO, intent reuse, DMA, retry or cleanup. Timeout is a bounded observation, not global service absence or a cause diagnosis.' if os.environ.get('SUNFISH_QRTR_WAIT_PROBE')=='1' else None,
    'qrtr_credit_probe':os.environ.get('SUNFISH_QRTR_CREDIT_PROBE')=='1',
    'qrtr_credit_scope':'Bounded NEW_SERVER inventory with fresh local IDs3..34 and32 disjoint private guarded buffers. After each validated DATA completion is consumed, inspect a stable empty RX ring or one exact request7/remote2/52. Empty ring permits one fresh16-byte INTENT with no ACK or RXtail store; a present validated request permits24-byte INTENT+ACK and consumes exactly8 bytes. Closed internal plans bound dynamic cursors to TX1192/RX3312; no wire value becomes a write address. Late/malformed requests stop before credit head publication. Up to33 records/32 new packets,20seconds,64 additional notifications,1536 additional SMEM write bytes maximum. Final DATA uncompleted; preserve all payloads/guards, no intent reuse, extra QRTR beyond prerequisite HELLO, service/WLAN QMI, DMA, retry or cleanup. Service ID0x45 only is a discovery marker, not compatible-version or endpoint authority.' if os.environ.get('SUNFISH_QRTR_CREDIT_PROBE')=='1' else None,
    'qrtr_discovery_probe':os.environ.get('SUNFISH_QRTR_DISCOVERY_PROBE')=='1',
    'qrtr_discovery_scope':'Passive inventory of up to33 NEW_SERVER records including retained intent2; stop at service ID0x45 or32 fresh packets. Owned verified72-byte data checkpoint; current suffix unclassified until exact request7/remote2/52 validation. At most32 fixed RXDONE6 and fresh advertisements/positiveACKs using distinct local IDs3..34 and32 private guarded buffers. Bounded TX1192/RX3312,20-second budget; full TX/live ready/identities/cursors/private payloads rechecked around each publication. No reread of released RX, final DATA uncompleted, no intent reuse, outgoing QRTR beyond prerequisite HELLO, service QMI, host DMA, retry or cleanup. Service ID is discovery evidence only; no compatible-version/endpoint authority.' if os.environ.get('SUNFISH_QRTR_DISCOVERY_PROBE')=='1' else None,
    'qrtr_next_probe':os.environ.get('SUNFISH_QRTR_NEXT_PROBE')=='1',
    'qrtr_next_scope':'Owned actual HELLO checkpoint and16-byte completion/request capture gate fresh local intent2 plus positive ACK:24 fixed TX bytes at144/head168, one seventh notification. Retain first buffer and reserve a disjoint guarded64-byte next buffer through restart. Preserve RXtail656 and pending16 bytes; double-copy at most80 new bytes at RX672, accept one DATA9/remote2/intent2 chunk52/left0 frame, copy52 and verify twice with both buffers/guards/zero remainder and final live checks. Decode header and all five body words diagnostically. No further RXDONE, peer intent reuse, extra QRTR TX, service/WLAN QMI, DMA, retry or cleanup' if os.environ.get('SUNFISH_QRTR_NEXT_PROBE')=='1' else None,
    'qrtr_hello_scope':'Owned actual completed receive gates current advertised peer intent0 with capacity at least52. One fixed72-byte GLINK DATA9/local1/peer-intent0 containing QRTR v1 HELLO node1 to0/control ports,52 bytes and zero padding, appended at TX72/head144; one sixth notification. Preserve RXtail656 and private buffer, double-copy at most512 control bytes, require full snapshot and exact RX_DONE6 or14/remote2/intent0 plus TXtail144. Report new intents/requests without using/answering. No reuse, receive advertisement/tail store, service query, WLAN/DMA, retry or cleanup' if os.environ.get('SUNFISH_QRTR_HELLO_PROBE')=='1' else None,
    'glink_done_scope':'Consume an owned actual guarded first receive, revalidate the complete648-byte queue and private52-byte payload twice. Stage fixed8-byte RX_DONE6/local1/intent1 at TX64 and verify all TX before one RXtail656 and TXhead72 store, then one fifth GLINK notification. Never reread consumed peer bytes. Wait for TXtail72 with ready/identity/TX/private-buffer checks, retain later RX suffix and private buffer lease through restart. No reuse/new intent, outgoing QRTR, WLAN, host DMA, retry or cleanup' if os.environ.get('SUNFISH_GLINK_DONE_PROBE')=='1' else None,
    'glink_rx_packet_scope':'Owned actual complete intent checkpoint gates one static guarded64-byte local buffer, fixed intent1 plus request ACK:24 scalar bytes at TX40/head64 and one bit12 notification. Preserve all576 unread bytes and RX tail8; wait for one9/remote2/intent1 chunk52/left0 frame, bounded80-byte double copies, copy52 into reserved private buffer and verify guards/remainder/ready/state. No buffer address transmitted, RX_DONE, receive consumption, peer-intent selection, outgoing QRTR payload, WLAN or DMA; retain unique static buffer lease through restart' if os.environ.get('SUNFISH_GLINK_RX_PACKET_PROBE')=='1' else None,
    'glink_intents_revision':2 if os.environ.get('SUNFISH_GLINK_INTENTS_PROBE')=='1' else None,
    'glink_intents_scope':'Revision2 of the same actual fixed version/OPEN/peer-ACK exchange and512-byte read-only snapshot. Retain whole-record prefix stats and exact rejected offset/header/preview. Decode up to64 diagnostic intent pairs/eight size groups, and observe header-only request7 without answering it. Preserve RX tail8, prior88 bytes and native writer. No intent selection, remote-size allocation, receive consumption, extra packet/index/notification writes, QRTR/WLAN/DMA' if os.environ.get('SUNFISH_GLINK_INTENTS_PROBE')=='1' else None,
    'glink_channel_scope':'Consume actual successful OPEN continuation, preserve first88 unread RX bytes and RX tail8, append fixed OPEN_ACK4/remote2/0 at TX32, publish head40 once and notify bit12 once. Wait for TXtail40 and inspect at most128 new control bytes twice; intent metadata stays opaque. No receive consumption, advertised intents, QRTR payload, WLAN or host DMA' if os.environ.get('SUNFISH_GLINK_CHANNEL_PROBE')=='1' else None,
    'glink_open_revision':3 if os.environ.get('SUNFISH_GLINK_OPEN_PROBE')=='1' else None,
    'glink_open_plan_snapshot':os.environ.get('SUNFISH_GLINK_OPEN_PLAN_SNAPSHOT')=='1',
    'glink_open_probe':os.environ.get('SUNFISH_GLINK_OPEN_PROBE')=='1' and os.environ.get('SUNFISH_GLINK_OPEN_PLAN_SNAPSHOT')!='1',
    'glink_open_plan_snapshot_scope':'After the fixed GLINK VERSION exchange, retain the stable bounded RX checkpoint, display its raw captured bytes, then stop before any OPEN payload byte, TX head store, OPEN notification or RX-tail store. No QRTR/QMI payload or WLAN/host DMA setup.' if os.environ.get('SUNFISH_GLINK_OPEN_PLAN_SNAPSHOT')=='1' else None,
    'glink_open_scope':'Private actual version checkpoint plus live queue structure and exact target-name IPCRTR offer check gate one fixed16-byte OPEN local ID1. The offer may be remote ID1 or ID2 in either observed queue order; the unused name need not match a policy literal and its live bytes remain unchanged. AP head32, RX tail8, one extra bit12 notification, bounded reply scan, guard-stage diagnostics. No peer OPEN_ACK, receive consumption, intent advertisement, QRTR payload, WLAN or DMA' if os.environ.get('SUNFISH_GLINK_OPEN_PROBE')=='1' else None,
    'glink_pending_scope':'After the unchanged fixed16-byte version send and one notification, copy up to128 pending RX bytes twice between exact descriptor/identity and ready checks. Parse only VERSION, VERSION_ACK, OPEN with bounded printable name and READ_NOTIF. Stop on unknown/truncated input; do not consume RX, acknowledge/open a channel or send another packet' if os.environ.get('SUNFISH_GLINK_PENDING_PROBE')=='1' else None,
    'glink_boot_scope':'Private wrapper owns the real AuthOnce through fresh allocation/pre-AUTH revalidation and one authentication/reset. After status-ready, consume sealed boot identity/ready evidence to compare descriptor identities and zero TX bytes without post-AUTH GLINK writes, packets, notifications/ACK, IRQ, WLAN or DMA setup' if os.environ.get('SUNFISH_GLINK_BOOT_PROBE')=='1' else None,
    'glink_receive_scope':'Read-only32-byte descriptor and at most one8-byte pending RX header twice; unchanged mappings; no FIFO advance, allocation, reset, GLINK notification/version ACK, IRQ, WLAN or host DMA setup' if os.environ.get('SUNFISH_GLINK_RECEIVE_PROBE')=='1' else None,
    'loader_hold_after_relocation':os.environ.get('SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION')=='1',
    'loader_hold_before_relocation':os.environ.get('SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION')=='1',
    'loader_hold_after_copy':os.environ.get('SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY')=='1',
    'loader_copy_progress':os.environ.get('SUNFISH_IMAGE_LOADER_COPY_PROGRESS')=='1',
    'loader_copy_inputs':os.environ.get('SUNFISH_IMAGE_LOADER_COPY_INPUTS')=='1',
    'loader_relocation_progress':os.environ.get('SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS')=='1',
    'loader_task_images':os.environ.get('SUNFISH_IMAGE_LOADER_TASK_IMAGES')=='1',
    'loader_boot_diagnostic':os.environ.get('SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC')=='1',
    'loader_stage_modem_image':os.environ.get('SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE')=='1',
    'loader_authenticate_modem':os.environ.get('SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM')=='1',
    'loader_observe_modem_ready':os.environ.get('SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY')=='1',
    'loader_handshake_modem_status':os.environ.get('SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS')=='1',
    'source':'Installed Pixel MDT and all29 loadable segment files; private read-only ADB copies',
    'metadata_bytes':len(b),'metadata_sha256':hashlib.sha256(b).hexdigest(),'program_headers':31,'loadable_segments':29,
    'modem_base':'0x8b000000','modem_end':'0x94800000','mapped_modem_bytes':0x9800000,
    'file_bytes':68393962,'zero_fill_bytes':89350155,'loaded_memory_bytes':157744117,
    'gap_snapshot_bytes':1639435,'owned_snapshot_capacity':0x200000,'sources':files,
    'executable_segment_contents_staged_in_nx_memory':os.environ['SUNFISH_MODEM_IMAGE_BIN']!='rust_sunfish_modem_image_boot_check',
    'modem_memory_access':os.environ['SUNFISH_MODEM_IMAGE_BIN']!='rust_sunfish_modem_image_boot_check',
    'firmware_execution_observed':False,'auth_or_reset':os.environ.get('SUNFISH_GLINK_PREPARE_PROBE')!='1' and os.environ['SUNFISH_MODEM_IMAGE_BIN'] in ['rust_sunfish_modem_auth_probe','rust_sunfish_modem_ready_probe','rust_sunfish_modem_smp2p_probe'],
    'scope':('One-shot fixed VERSION_ACK and VERSION1 with local features0 after native-verified fresh preparation/AUTH/ready and exact peer offer0/1/7; verify sixteen TX bytes before one TX head16 and RX tail8 publication, notify bit12 once, poll read-only for consumption and ACK; no channel, IRQ, WLAN, DMA, retry or cleanup' if os.environ.get('SUNFISH_GLINK_VERSION_PROBE')=='1' else ('Repeat native-verified fresh GLINK allocation before AUTH using a private session-owning wrapper; revalidate memory and fresh metadata before one AUTH/reset, then read-only compare retained GLINK identity, AP indices, zero TX bytes and receive state after ready; no post-AUTH GLINK writes, packets, notifications/ACK, IRQ, WLAN or host DMA setup' if os.environ.get('SUNFISH_GLINK_BOOT_PROBE')=='1' else ('Stop before AUTH: consume the staged/INFO/fresh-metadata session, require exact AP-only preauth partition, initialize fresh descriptor478 and TX479, verify bytes and untouched guards, commit the cursor once; five RW SMEM pages, no authentication/reset, modem start, GLINK packet/notification, WLAN or host DMA setup' if os.environ.get('SUNFISH_GLINK_PREPARE_PROBE')=='1' else ('After full staging, INFO availability and fresh metadata gates, publish one AP status item in the empty partition before AUTH; reinspect metadata, authenticate once, then notify only after a new valid peer header. After sealed ready observation, '+('compare allocation metadata and two32-byte descriptor/at-most8-byte pending RX-header snapshots' if os.environ.get('SUNFISH_GLINK_RECEIVE_PROBE')=='1' else 'compare two GLINK allocation-directory snapshots without reading GLINK payloads')+'. Optional SSR ACK feature disabled; no GLINK writes, WLAN QMI or host DMA setup' if os.environ['SUNFISH_MODEM_IMAGE_BIN']=='rust_sunfish_modem_smp2p_probe' else ('Repeat staging and one-shot PAS4 authentication/reset, then passively observe a newly published modem SMP2P ready bit; read-only SMEM with lock3 synchronization, no AP transport writes, WLAN QMI or host DMA setup' if os.environ['SUNFISH_MODEM_IMAGE_BIN']=='rust_sunfish_modem_ready_probe' else ('Stage and verify all data/BSS/gaps, then one fixed PAS4 authentication/reset after fresh INFO and metadata gates; report only secure results, no ready claim or WLAN/DMA setup' if os.environ['SUNFISH_MODEM_IMAGE_BIN']=='rust_sunfish_modem_auth_probe' else ('Read the complete immutable payload in owned ELF RAM only,with visible loader markers; no modem memory,secure call or resource vote' if os.environ['SUNFISH_MODEM_IMAGE_BIN']=='rust_sunfish_modem_image_boot_check' else 'Stage all file bytes and exact BSS,verify every loaded byte twice and compare exact gap snapshots; no authentication/reset/modem start/DMA')))))))
}
if os.environ.get('SUNFISH_GLINK_OPEN_PROBE')=='1':
    profile['scope']=profile['glink_open_scope']
if os.environ.get('SUNFISH_GLINK_OPEN_PLAN_SNAPSHOT')=='1':
    profile['scope']=profile['glink_open_plan_snapshot_scope']
if os.environ.get('SUNFISH_GLINK_CHANNEL_PROBE')=='1':
    profile['scope']=profile['glink_channel_scope']
if os.environ.get('SUNFISH_GLINK_INTENTS_PROBE')=='1':
    profile['scope']=profile['glink_intents_scope']
if os.environ.get('SUNFISH_GLINK_RX_PACKET_PROBE')=='1':
    profile['scope']=profile['glink_rx_packet_scope']
if os.environ.get('SUNFISH_GLINK_DONE_PROBE')=='1':
    profile['scope']=profile['glink_done_scope']
if os.environ.get('SUNFISH_QRTR_HELLO_PROBE')=='1':
    profile['scope']=profile['qrtr_hello_scope']
if os.environ.get('SUNFISH_QRTR_NEXT_PROBE')=='1':
    profile['scope']=profile['qrtr_next_scope']
if os.environ.get('SUNFISH_QRTR_DISCOVERY_PROBE')=='1':
    profile['scope']=profile['qrtr_discovery_scope']
if os.environ.get('SUNFISH_QRTR_CREDIT_PROBE')=='1':
    profile['scope']=profile['qrtr_credit_scope']
if os.environ.get('SUNFISH_QRTR_WAIT_PROBE')=='1':
    profile['scope']=profile['qrtr_wait_scope']
if os.environ.get('SUNFISH_QRTR_SERVREG_PROBE')=='1':
    if os.environ.get('SUNFISH_QRTR_SERVREG_PDR_FOLLOWUP')=='1':
        profile['scope']=profile['qrtr_servreg_pdr_followup_scope']
    elif os.environ.get('SUNFISH_QRTR_SERVREG_PDR_REPLY')=='1':
        profile['scope']=profile['qrtr_servreg_pdr_reply_scope']
    elif os.environ.get('SUNFISH_QRTR_SERVREG_QUERY_CHECK')=='1':
        profile['scope']=profile['qrtr_servreg_query_check_scope']
    elif os.environ.get('SUNFISH_QRTR_SERVREG_PREQUERY_CHECK')=='1':
        profile['scope']=profile['qrtr_servreg_prequery_check_scope']
    else:
        profile['scope']=profile['qrtr_servreg_scope']
if os.environ.get('SUNFISH_MODEM_SERVREG_RESTART_PROBE')=='1':
    profile['scope']=profile['modem_servreg_restart_probe_scope']
elif os.environ.get('SUNFISH_MODEM_TFTP_OBSERVER')=='1':
    profile['scope']=profile['modem_tftp_observer_scope']
elif os.environ.get('SUNFISH_MODEM_SERVREG_STATE_PROBE')=='1':
    profile['scope']=profile['modem_servreg_state_probe_scope']
if os.environ.get('SUNFISH_MODEM_CRASH_PROBE')=='1':
    profile['scope']=profile['modem_crash_scope']
if os.environ.get('SUNFISH_MODEM_FATAL_TRACE_PROBE')=='1':
    profile['scope']=profile['modem_fatal_trace_scope']
Path(os.environ['SUNFISH_MODEM_IMAGE_PROFILE']).write_text(json.dumps(profile,indent=2)+'\n')
PY
if [ "${SUNFISH_QRTR_SERVREG_PDR_FOLLOWUP:-0}" = 1 ]; then
    python3 "$port_dir/validate-qrtr-pdr-followup-inputs.py" > "$out_dir/qrtr-pdr-followup-input-proof.json"
fi
python3 -m unittest tests.sunfish_image_boot_layout_test tests.sunfish_boot_v2_packer_test tests.sunfish_pas_metadata_probe_test tests.sunfish_modem_prepare_probe_test tests.sunfish_pas_mem_setup_probe_test tests.sunfish_pas_init_probe_test tests.sunfish_modem_data_probe_test tests.sunfish_modem_segment_probe_test tests.sunfish_modem_image_probe_test
rustc +stable --edition 2024 --test tests/sunfish_pas_smc_adapter_test.rs \
    -o "$out_dir/pas-smc-adapter-test"
"$out_dir/pas-smc-adapter-test" --quiet
if [ "${SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM:-0}" = 1 ] || [ "${SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY:-0}" = 1 ] || [ "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1 ]; then
    rustc +stable --edition 2024 --test "$root_dir/tests/sunfish_pas_auth_test.rs" -o "$out_dir/auth-test"
    "$out_dir/auth-test" --quiet
    python3 "$port_dir/validate-modem-auth-inputs.py" > "$out_dir/auth-input-proof.json"
fi
if [ "${SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY:-0}" = 1 ] || [ "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1 ]; then
    rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_modem_status_test.rs" -o "$out_dir/status-test"
    "$out_dir/status-test" --quiet
    python3 "$port_dir/validate-modem-ready-inputs.py" > "$out_dir/ready-input-proof.json"
    python3 -m unittest tests.sunfish_modem_ready_probe_test
fi
if [ "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1 ]; then
    rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_smp2p_handshake_test.rs" -o "$out_dir/handshake-test"
    "$out_dir/handshake-test" --quiet
    python3 "$port_dir/validate-smp2p-handshake-inputs.py" > "$out_dir/handshake-input-proof.json"
    python3 -m unittest tests.sunfish_smp2p_probe_test
    rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_inventory_test.rs" -o "$out_dir/glink-inventory-test"
    "$out_dir/glink-inventory-test" --quiet
    python3 "$port_dir/validate-glink-directory-inputs.py" > "$out_dir/glink-directory-input-proof.json"
    if [ "${SUNFISH_GLINK_RECEIVE_PROBE:-0}" = 1 ]; then
        rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_receive_test.rs" -o "$out_dir/glink-receive-test"
        "$out_dir/glink-receive-test" --quiet
        python3 "$port_dir/validate-glink-receive-inputs.py" > "$out_dir/glink-receive-input-proof.json"
    fi
    if [ "${SUNFISH_GLINK_PREPARE_PROBE:-0}" = 1 ] || [ "${SUNFISH_GLINK_BOOT_PROBE:-0}" = 1 ] || [ "${SUNFISH_GLINK_VERSION_PROBE:-0}" = 1 ]; then
        rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_prepare_test.rs" -o "$out_dir/glink-prepare-test"
        "$out_dir/glink-prepare-test" --quiet
        python3 "$port_dir/validate-glink-prepare-inputs.py" > "$out_dir/glink-prepare-input-proof.json"
        python3 -m unittest tests.sunfish_glink_prepare_probe_test
    fi
    if [ "${SUNFISH_GLINK_BOOT_PROBE:-0}" = 1 ] || [ "${SUNFISH_GLINK_VERSION_PROBE:-0}" = 1 ]; then
        rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_boot_test.rs" -o "$out_dir/glink-boot-test"
        "$out_dir/glink-boot-test" --quiet
        python3 "$port_dir/validate-glink-boot-inputs.py" > "$out_dir/glink-boot-input-proof.json"
    fi
    if [ "${SUNFISH_GLINK_VERSION_PROBE:-0}" = 1 ]; then
        rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_version_test.rs" -o "$out_dir/glink-version-test"
        "$out_dir/glink-version-test" --quiet
        python3 "$port_dir/validate-glink-version-inputs.py" > "$out_dir/glink-version-input-proof.json"
        if [ "${SUNFISH_GLINK_PENDING_PROBE:-0}" = 1 ]; then
            rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_pending_test.rs" -o "$out_dir/glink-pending-test"
            "$out_dir/glink-pending-test" --quiet
            python3 "$port_dir/validate-glink-pending-inputs.py" > "$out_dir/glink-pending-input-proof.json"
            if [ "${SUNFISH_GLINK_OPEN_PROBE:-0}" = 1 ]; then
                rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_open_test.rs" -o "$out_dir/glink-open-test"
                "$out_dir/glink-open-test" --quiet
                python3 "$port_dir/validate-glink-open-inputs.py" > "$out_dir/glink-open-input-proof.json"
                if [ "${SUNFISH_GLINK_CHANNEL_PROBE:-0}" = 1 ]; then
                    rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_channel_test.rs" -o "$out_dir/glink-channel-test"
                    "$out_dir/glink-channel-test" --quiet
                    python3 "$port_dir/validate-glink-channel-inputs.py" > "$out_dir/glink-channel-input-proof.json"
                    if [ "${SUNFISH_GLINK_INTENTS_PROBE:-0}" = 1 ]; then
                        rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_intents_test.rs" --cfg 'feature="sunfish-glink-intents-probe"' -o "$out_dir/glink-intents-test"
                        "$out_dir/glink-intents-test" --quiet
                        python3 "$port_dir/validate-glink-intents-inputs.py" > "$out_dir/glink-intents-input-proof.json"
                        if [ "${SUNFISH_GLINK_RX_PACKET_PROBE:-0}" = 1 ]; then
                            rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_rx_test.rs" --cfg 'feature="sunfish-glink-intents-probe"' --cfg 'feature="sunfish-glink-rx-packet-probe"' -o "$out_dir/glink-rx-test"
                            "$out_dir/glink-rx-test" --quiet
                            python3 "$port_dir/validate-glink-rx-inputs.py" > "$out_dir/glink-rx-input-proof.json"
                            if [ "${SUNFISH_GLINK_DONE_PROBE:-0}" = 1 ]; then
                                rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_glink_done_test.rs" --cfg 'feature="sunfish-glink-intents-probe"' --cfg 'feature="sunfish-glink-rx-packet-probe"' --cfg 'feature="sunfish-glink-done-probe"' -o "$out_dir/glink-done-test"
                                "$out_dir/glink-done-test" --quiet
                                python3 "$port_dir/validate-glink-done-inputs.py" > "$out_dir/glink-done-input-proof.json"
                                if [ "${SUNFISH_QRTR_HELLO_PROBE:-0}" = 1 ]; then
                                    rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_qrtr_hello_test.rs" --cfg 'feature="sunfish-glink-intents-probe"' --cfg 'feature="sunfish-glink-rx-packet-probe"' --cfg 'feature="sunfish-glink-done-probe"' --cfg 'feature="sunfish-qrtr-hello-probe"' -o "$out_dir/qrtr-hello-test"
                                    "$out_dir/qrtr-hello-test" --quiet
                                    python3 "$port_dir/validate-qrtr-hello-inputs.py" > "$out_dir/qrtr-hello-input-proof.json"
                                    if [ "${SUNFISH_QRTR_NEXT_PROBE:-0}" = 1 ]; then
                                        rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_qrtr_next_test.rs" --cfg 'feature="sunfish-glink-intents-probe"' --cfg 'feature="sunfish-glink-rx-packet-probe"' --cfg 'feature="sunfish-glink-done-probe"' --cfg 'feature="sunfish-qrtr-hello-probe"' --cfg 'feature="sunfish-qrtr-next-probe"' -o "$out_dir/qrtr-next-test"
                                        "$out_dir/qrtr-next-test" --quiet
                                        python3 "$port_dir/validate-qrtr-next-inputs.py" > "$out_dir/qrtr-next-input-proof.json"
                                        if [ "${SUNFISH_QRTR_DISCOVERY_PROBE:-0}" = 1 ]; then
                                            rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_qrtr_discovery_test.rs" --cfg 'feature="sunfish-glink-intents-probe"' --cfg 'feature="sunfish-glink-rx-packet-probe"' --cfg 'feature="sunfish-glink-done-probe"' --cfg 'feature="sunfish-qrtr-hello-probe"' --cfg 'feature="sunfish-qrtr-next-probe"' --cfg 'feature="sunfish-qrtr-discovery-probe"' -o "$out_dir/qrtr-discovery-test"
                                            "$out_dir/qrtr-discovery-test" --quiet
                                            python3 "$port_dir/validate-qrtr-discovery-inputs.py" > "$out_dir/qrtr-discovery-input-proof.json"
                                            if [ "${SUNFISH_QRTR_CREDIT_PROBE:-0}" = 1 ]; then
                                                rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_qrtr_credit_test.rs" --cfg 'feature="sunfish-glink-intents-probe"' --cfg 'feature="sunfish-glink-rx-packet-probe"' --cfg 'feature="sunfish-glink-done-probe"' --cfg 'feature="sunfish-qrtr-hello-probe"' --cfg 'feature="sunfish-qrtr-next-probe"' --cfg 'feature="sunfish-qrtr-discovery-probe"' --cfg 'feature="sunfish-qrtr-credit-probe"' -o "$out_dir/qrtr-credit-test"
                                                "$out_dir/qrtr-credit-test" --quiet
                                                python3 "$port_dir/validate-qrtr-credit-inputs.py" > "$out_dir/qrtr-credit-input-proof.json"
                                                if [ "${SUNFISH_QRTR_WAIT_PROBE:-0}" = 1 ]; then
                                                    rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_qrtr_wait_test.rs" --cfg 'feature="sunfish-glink-intents-probe"' --cfg 'feature="sunfish-glink-rx-packet-probe"' --cfg 'feature="sunfish-glink-done-probe"' --cfg 'feature="sunfish-qrtr-hello-probe"' --cfg 'feature="sunfish-qrtr-next-probe"' --cfg 'feature="sunfish-qrtr-discovery-probe"' --cfg 'feature="sunfish-qrtr-credit-probe"' --cfg 'feature="sunfish-qrtr-wait-probe"' -o "$out_dir/qrtr-wait-test"
                                                    "$out_dir/qrtr-wait-test" --quiet
                                                    python3 "$port_dir/validate-qrtr-wait-inputs.py" > "$out_dir/qrtr-wait-input-proof.json"
                                                    if [ "${SUNFISH_MODEM_CRASH_PROBE:-0}" = 1 ]; then
                                                        fatal_trace_cfg=
                                                        if [ "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" = 1 ]; then
                                                            fatal_trace_cfg='--cfg feature="sunfish-modem-fatal-trace-probe"'
                                                        fi
                                                        rustc +stable --edition 2024 -O --test "$root_dir/tests/sunfish_modem_crash_test.rs" --cfg 'feature="sunfish-glink-intents-probe"' --cfg 'feature="sunfish-glink-rx-packet-probe"' --cfg 'feature="sunfish-glink-done-probe"' --cfg 'feature="sunfish-qrtr-hello-probe"' --cfg 'feature="sunfish-qrtr-next-probe"' --cfg 'feature="sunfish-qrtr-discovery-probe"' --cfg 'feature="sunfish-qrtr-credit-probe"' --cfg 'feature="sunfish-qrtr-wait-probe"' --cfg 'feature="sunfish-modem-crash-probe"' $fatal_trace_cfg -o "$out_dir/modem-crash-test"
                                                        "$out_dir/modem-crash-test" --quiet
                                                        python3 "$port_dir/validate-modem-crash-inputs.py" > "$out_dir/modem-crash-input-proof.json"
                                                        if [ "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" = 1 ]; then
                                                            python3 "$port_dir/validate-modem-fatal-trace-inputs.py" > "$out_dir/modem-fatal-trace-input-proof.json"
                                                        fi
                                                    fi
                                                    if [ "${SUNFISH_QRTR_SERVREG_PROBE:-0}" = 1 ]; then
                                                        rustc +stable --edition 2024 --test "$root_dir/tests/sunfish_qrtr_snapshot_test.rs" -o "$out_dir/qrtr-snapshot-test"
                                                        "$out_dir/qrtr-snapshot-test" --quiet
                                                        rustc +stable --edition 2024 --test "$root_dir/tests/sunfish_qrtr_servreg_probe_test.rs" -o "$out_dir/qrtr-servreg-prequery-test"
                                                        "$out_dir/qrtr-servreg-prequery-test" --quiet
                                                        rustc +stable --edition 2024 --test "$root_dir/rust/src/pixel_servreg_wire.rs" -o "$out_dir/servreg-wire-test"
                                                        "$out_dir/servreg-wire-test" --quiet
                                                    fi
                                                fi
                                            fi
                                        fi
                                    fi
                                fi
                            fi
                        fi
                    fi
                fi
            fi
        fi
    fi
fi
rustc +stable --edition 2024 --test tests/sunfish_aop_mmio_test.rs \
    -o "$out_dir/aop-mmio-test"
"$out_dir/aop-mmio-test" --quiet
rustc +stable --edition 2024 --test tests/sunfish_bcm_reads_mmio_test.rs \
    -o "$out_dir/bcm-mmio-test"
"$out_dir/bcm-mmio-test" --quiet
rustc +stable --edition 2024 --test tests/sunfish_pas_metadata_mmio_test.rs \
    -o "$out_dir/metadata-mmio-test"
"$out_dir/metadata-mmio-test" --quiet
rustc +stable --edition 2024 -O --test tests/sunfish_modem_image_mmio_test.rs \
    -o "$out_dir/data-mmio-test"
"$out_dir/data-mmio-test" --quiet
rustc +stable --edition 2024 --test rust/src/freestanding.rs -o "$out_dir/freestanding-test"
"$out_dir/freestanding-test" --quiet
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --features "$image_features" \
    --bin "$image_bin" \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp "rust/target/aarch64-sel4-microkit-pixel4k/release/$image_bin.elf" \
    "$out_dir/modem_image_probe.elf"
if [ "${SUNFISH_IMAGE_LOADER_CHECKPOINTS:-0}" = 1 ]; then
    clang -std=c11 -Wall -Wextra -Werror "$root_dir/tests/sunfish_loader_checkpoint_test.c" -o "$out_dir/loader-checkpoint-test"
    "$out_dir/loader-checkpoint-test"
    if [ "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY:-0}" = 1 ]; then
        clang -std=c11 -Wall -Wextra -Werror "$root_dir/tests/sunfish_loader_copy_test.c" -o "$out_dir/loader-copy-test"
        "$out_dir/loader-copy-test"
        if [ "${SUNFISH_IMAGE_LOADER_COPY_PROGRESS:-0}" = 1 ]; then
            clang -std=c11 -O2 -Wall -Wextra -Werror "$root_dir/tests/sunfish_loader_copy_progress_test.c" -o "$out_dir/loader-copy-progress-test"
            "$out_dir/loader-copy-progress-test"
        fi
        if [ "${SUNFISH_IMAGE_LOADER_COPY_INPUTS:-0}" = 1 ]; then
            clang -std=c11 -O2 -Wall -Wextra -Werror "$root_dir/tests/sunfish_loader_copy_inputs_test.c" -o "$out_dir/loader-copy-inputs-test"
            "$out_dir/loader-copy-inputs-test"
        fi
    fi
    if [ "${SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS:-0}" = 1 ]; then
        clang -std=c11 -O2 -Wall -Wextra -Werror "$root_dir/tests/sunfish_loader_copy_progress_test.c" -o "$out_dir/loader-copy-progress-test"
        "$out_dir/loader-copy-progress-test"
    fi
    if [ "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_RELOCATION:-0}" = 1 ]; then
        if [ "${SUNFISH_IMAGE_LOADER_RELOCATION_PROGRESS:-0}" = 1 ]; then
            if [ "${SUNFISH_IMAGE_LOADER_TASK_IMAGES:-0}" = 1 ]; then
                set -- --out-dir "$out_dir" --checkpoints --hold-after-relocation --relocation-progress --task-images
                if [ "${SUNFISH_IMAGE_LOADER_BOOT_DIAGNOSTIC:-0}" = 1 ]; then
                    set -- "$@" --boot-diagnostic
                fi
                if [ "${SUNFISH_IMAGE_LOADER_STAGE_MODEM_IMAGE:-0}" = 1 ]; then
                    set -- "$@" --stage-modem-image
                fi
                if [ "${SUNFISH_IMAGE_LOADER_AUTHENTICATE_MODEM:-0}" = 1 ]; then
                    set -- "$@" --authenticate-modem
                fi
                if [ "${SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY:-0}" = 1 ]; then
                    set -- "$@" --observe-modem-ready
                fi
                if [ "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1 ]; then
                    if [ "${SUNFISH_MODEM_SERVREG_STATE_PROBE:-0}" = 1 ]; then
                        set -- "$@" --modem-servreg-state-probe
                    elif [ "${SUNFISH_QRTR_SERVREG_PROBE:-0}" = 1 ]; then
                        set -- "$@" --qrtr-servreg-probe
                    else
                        set -- "$@" --handshake-modem-status
                    fi
                fi
                if [ "${SUNFISH_MODEM_SERVREG_RESTART_PROBE:-0}" = 1 ]; then
                    set -- "$@" --modem-servreg-restart-probe
                fi
                if [ "${SUNFISH_MODEM_TFTP_OBSERVER:-0}" = 1 ]; then
                    set -- "$@" --modem-tftp-observer
                fi
                if [ "${SUNFISH_QRTR_SERVREG_PDR_FOLLOWUP:-0}" = 1 ]; then
                    set -- "$@" --qrtr-servreg-pdr-followup
                fi
                if [ "${SUNFISH_QRTR_SERVREG_PDR_REPLY:-0}" = 1 ] && [ "${SUNFISH_QRTR_SERVREG_PDR_FOLLOWUP:-0}" != 1 ]; then
                    set -- "$@" --qrtr-servreg-pdr-reply
                fi
                # SERVREG has a dedicated task manifest and runs from the
                # validated prerequisite checkpoint. The GLINK/QRTR flags set
                # by its wrapper describe compiled dependencies and offline
                # checks, not additional loader probe modes. Passing them here
                # combines mutually exclusive manifest selectors.
                if [ "${SUNFISH_QRTR_SERVREG_PROBE:-0}" != 1 ] && [ "${SUNFISH_MODEM_SERVREG_STATE_PROBE:-0}" != 1 ] && [ "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" != 1 ]; then
                    if [ "${SUNFISH_GLINK_OPEN_PROBE:-0}" = 1 ] && [ "${SUNFISH_GLINK_OPEN_PLAN_SNAPSHOT:-0}" = 0 ]; then
                        set -- "$@" --glink-open-probe
                    fi
                    if [ "${SUNFISH_GLINK_CHANNEL_PROBE:-0}" = 1 ]; then
                        set -- "$@" --glink-channel-probe
                    fi
                    if [ "${SUNFISH_GLINK_INTENTS_PROBE:-0}" = 1 ]; then
                        set -- "$@" --glink-intents-probe
                    fi
                    if [ "${SUNFISH_GLINK_RX_PACKET_PROBE:-0}" = 1 ]; then
                        set -- "$@" --glink-rx-packet-probe
                    fi
                    if [ "${SUNFISH_GLINK_DONE_PROBE:-0}" = 1 ]; then
                        set -- "$@" --glink-done-probe
                    fi
                    if [ "${SUNFISH_QRTR_HELLO_PROBE:-0}" = 1 ]; then
                        set -- "$@" --qrtr-hello-probe
                    fi
                    if [ "${SUNFISH_QRTR_NEXT_PROBE:-0}" = 1 ]; then
                        set -- "$@" --qrtr-next-probe
                    fi
                    if [ "${SUNFISH_QRTR_DISCOVERY_PROBE:-0}" = 1 ]; then
                        set -- "$@" --qrtr-discovery-probe
                    fi
                    if [ "${SUNFISH_QRTR_CREDIT_PROBE:-0}" = 1 ]; then
                        set -- "$@" --qrtr-credit-probe
                    fi
                    if [ "${SUNFISH_QRTR_WAIT_PROBE:-0}" = 1 ]; then
                        set -- "$@" --qrtr-wait-probe
                    fi
                    if [ "${SUNFISH_GLINK_OPEN_PLAN_SNAPSHOT:-0}" = 1 ]; then
                        set -- "$@" --glink-open-plan-snapshot
                    fi
                fi
                if [ "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" = 1 ]; then
                    set -- "$@" --fatal-trace
                fi
                python3 "$port_dir/prepare-image-loader-sdk.py" "$@"
                clang -std=c11 -O2 -Wall -Wextra -Werror -fsanitize=address,undefined -I"$out_dir/loader-checkpoint-source/src" "$root_dir/tests/sunfish_loader_task_images_test.c" -o "$out_dir/loader-task-images-test"
                "$out_dir/loader-task-images-test"
            else
                python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out_dir" --checkpoints --hold-after-relocation --relocation-progress
            fi
        else
            python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out_dir" --checkpoints --hold-after-relocation
        fi
    elif [ "${SUNFISH_IMAGE_LOADER_HOLD_BEFORE_RELOCATION:-0}" = 1 ]; then
        python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out_dir" --checkpoints --hold-before-relocation
    elif [ "${SUNFISH_IMAGE_LOADER_HOLD_AFTER_COPY:-0}" = 1 ]; then
        if [ "${SUNFISH_IMAGE_LOADER_COPY_INPUTS:-0}" = 1 ]; then
            python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out_dir" --checkpoints --hold-after-copy --copy-inputs
        elif [ "${SUNFISH_IMAGE_LOADER_COPY_PROGRESS:-0}" = 1 ]; then
            python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out_dir" --checkpoints --hold-after-copy --copy-progress
        else
            python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out_dir" --checkpoints --hold-after-copy
        fi
    else
        python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out_dir" --checkpoints
    fi
else
    python3 "$port_dir/prepare-image-loader-sdk.py" --out-dir "$out_dir"
fi
MICROKIT_SDK="$out_dir/sdk" "$sdk/bin/microkit" "$system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
set --
if [ "${SUNFISH_IMAGE_LOADER_OBSERVE_MODEM_READY:-0}" = 1 ]; then
    set -- --observe-modem-ready
fi
if [ "${SUNFISH_IMAGE_LOADER_HANDSHAKE_MODEM_STATUS:-0}" = 1 ]; then
    set -- --handshake-modem-status
fi
if [ "${SUNFISH_GLINK_PREPARE_PROBE:-0}" = 1 ] || [ "${SUNFISH_GLINK_BOOT_PROBE:-0}" = 1 ] || [ "${SUNFISH_GLINK_VERSION_PROBE:-0}" = 1 ] || [ "${SUNFISH_QRTR_SERVREG_PROBE:-0}" = 1 ] || [ "${SUNFISH_MODEM_SERVREG_STATE_PROBE:-0}" = 1 ] || [ "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" = 1 ]; then
    set -- "$@" --prepare-glink-memory
fi
if [ "${SUNFISH_MODEM_FATAL_TRACE_PROBE:-0}" = 1 ]; then
    set -- "$@" --fatal-trace
fi
if [ "${SUNFISH_MODEM_SERVREG_STATE_PROBE:-0}" = 1 ] || [ "${SUNFISH_MODEM_SERVREG_RESTART_PROBE:-0}" = 1 ]; then
    set -- "$@" --servreg-state-probe
fi
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt" \
    --fixed-modem-image-system "$system" "$@"
python3 "$port_dir/validate-pas-metadata-layout.py" --report "$out_dir/report.txt" \
    > "$out_dir/metadata-layout.json"
if [ "${SUNFISH_MODEM_SERVREG_RESTART_PROBE:-0}" = 1 ]; then
    python3 "$port_dir/validate-modem-image-layout.py" --report "$out_dir/report.txt" \
        --system "$system" "$@" --servreg-restart-probe \
        > "$out_dir/modem-reservation-proof.json"
else
    python3 "$port_dir/validate-modem-image-layout.py" --report "$out_dir/report.txt" \
        --system "$system" "$@" \
        > "$out_dir/modem-reservation-proof.json"
fi
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/modem-image-probe.Image"
python3 - "$out_dir/modem-image-probe.Image" <<'PY'
import sys
from pathlib import Path
size=Path(sys.argv[1]).stat().st_size
assert size <= 0x5000000+4096, 'wrapped image exceeds the bounded loader copy'
print('Wrapped image size:',size,'bytes; full placement is checked after packing')
PY
lz4 -z -f -q "$out_dir/modem-image-probe.Image" "$out_dir/modem-image-probe.Image.lz4"
python3 - "$out_dir/minimal-ramdisk.cpio.gz" <<'PY'
import gzip,sys
from pathlib import Path
name=b'TRAILER!!!\0'
fields=[0,0,0,0,1,0,0,0,0,0,0,len(name),0]
archive=b'070701'+b''.join(('%08x'%n).encode() for n in fields)+name
archive+=b'\0'*(-len(archive)%4)
Path(sys.argv[1]).write_bytes(gzip.compress(archive,mtime=0))
PY
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/modem-image-probe.Image.lz4" "$out_dir/fastboot.img" \
    --ramdisk "$out_dir/minimal-ramdisk.cpio.gz"
python3 "$port_dir/validate-image-boot-layout.py" --out-dir "$out_dir" > "$out_dir/boot-layout-proof.json"
printf 'Built temporary probe %s locally: %s\n' "$image_bin" "$out_dir/fastboot.img"
