#!/bin/sh
set -eu
root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
port_dir="$root_dir/platforms/sunfish"
out_dir="$root_dir/.cache/pixel-sunfish/modem-bcm-after-aop-probe"
sdk="$root_dir/.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish"
reference="$HOME/Backups/pixel-sunfish-20261006/live-slots/boot_a.img"
export PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/bin:$PATH"
export SUNFISH_MODEM_MDT_PATH="$root_dir/.cache/pixel-sunfish/pas-metadata-prepare/modem.mdt"
test -f "$reference"
test -f "$SUNFISH_MODEM_MDT_PATH"
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
Path('.cache/pixel-sunfish/modem-bcm-after-aop-probe/metadata-profile.json').write_text(json.dumps({
    'source':'Installed Pixel MDT header only, not executable segment files',
    'bytes':len(b),'sha256':record['sha256'],'program_headers':31,'loadable_segments':29,
    'linked_start':'0x8b000000','linked_end':'0x94800000',
    'modem_pool_mapped':False,'secure_operation_invoked':False,
    'scope':'Pinned metadata preparation only; probe performs AOP ON and two read-only BCM passes, no resource WRITE, modem start, or WLAN DMA'
},indent=2)+'\n')
PY
python3 -m unittest tests.sunfish_pas_metadata_probe_test tests.sunfish_modem_prepare_probe_test tests.sunfish_modem_bcm_after_aop_probe_test
rustc +stable --edition 2024 --test tests/sunfish_aop_mmio_test.rs \
    -o "$out_dir/aop-mmio-test"
"$out_dir/aop-mmio-test" --quiet
rustc +stable --edition 2024 --test tests/sunfish_bcm_reads_mmio_test.rs \
    -o "$out_dir/bcm-reads-mmio-test"
"$out_dir/bcm-reads-mmio-test" --quiet
rustc +stable --edition 2024 --test tests/sunfish_pas_metadata_mmio_test.rs \
    -o "$out_dir/metadata-mmio-test"
"$out_dir/metadata-mmio-test" --quiet
SEL4_INCLUDE_DIRS="$sdk/board/sunfish/debug/include" \
    cargo +nightly-2026-09-25 build --locked --offline --release \
    --manifest-path rust/Cargo.toml --features sunfish-firmware-metadata-probe \
    --bin rust_sunfish_modem_bcm_after_aop_probe \
    --target rust/targets/aarch64-sel4-microkit-pixel4k.json \
    -Z build-std=core,alloc -Z json-target-spec
cp rust/target/aarch64-sel4-microkit-pixel4k/release/rust_sunfish_modem_bcm_after_aop_probe.elf \
    "$out_dir/modem_bcm_after_aop_probe.elf"
"$sdk/bin/microkit" "$port_dir/modem-bcm-after-aop-probe.system" \
    --search-path "$out_dir" --board sunfish --config debug \
    -o "$out_dir/loader.img" -r "$out_dir/report.txt"
python3 "$port_dir/audit-capdl-memory.py" --report "$out_dir/report.txt"
python3 "$port_dir/validate-pas-metadata-layout.py" --report "$out_dir/report.txt" \
    > "$out_dir/metadata-layout.json"
python3 "$port_dir/wrap-microkit-image.py" \
    "$out_dir/loader.img" "$out_dir/modem-bcm-after-aop-probe.Image"
lz4 -z -f -q "$out_dir/modem-bcm-after-aop-probe.Image" "$out_dir/modem-bcm-after-aop-probe.Image.lz4"
python3 "$port_dir/repack-boot-v2.py" \
    "$reference" "$out_dir/modem-bcm-after-aop-probe.Image.lz4" "$out_dir/fastboot.img"
printf 'Built two-pass BCM capture after AOP ON (no BCM resource WRITE): %s\n' "$out_dir/fastboot.img"
