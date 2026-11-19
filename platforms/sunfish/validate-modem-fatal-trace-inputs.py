#!/usr/bin/env python3
"""Pin read-only Atoll SMP2P fatal status capture and bounded reason snapshot."""
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET

PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
KERNEL=ROOT/'.cache/sunfish-lineage-kernel'
COMMIT='28f9290ae067d3e7857126eb6312720dbe9c7c80'

def main():
    prior_full=json.loads(subprocess.check_output(['python3',str(PORT/'validate-modem-crash-inputs.py')],text=True))
    prior={key:prior_full[key] for key in (
        'matched_kernel_base_commit','reason_item','global_hosts','reason_max_bytes',
        'reason_clear','post_terminal_protocol_action','extra_packet_or_write_authority',
        'reference_sha256','git_reference_sha256',
        'source_trace_not_running_binary_provenance')}
    native=prior_full['previous_native_measured_fatal']
    prior['previous_native_measured_fatal']={key:native[key] for key in (
        'date','artifact_sha256','native_result','error','guard_stage','records',
        'last_poll_ms','clock_hz','old_guards','pool_guards')}
    dts=(KERNEL/'arch/arm64/boot/dts/qcom/atoll.dtsi').read_text()
    modem=dts[dts.index('pil_modem: qcom,mss@4080000'):]
    modem=modem[:modem.index('\n\t};')]
    assert 'qcom,smem-id = <421>;' in modem and 'qcom,pas-id = <4>;' in modem
    for source in [
        '<&pdc GIC_SPI 266 IRQ_TYPE_EDGE_RISING>',
        '<&modem_smp2p_in 0 IRQ_TYPE_NONE>',
        '<&modem_smp2p_in 1 IRQ_TYPE_NONE>',
        '<&modem_smp2p_in 2 IRQ_TYPE_NONE>',
        '<&modem_smp2p_in 3 IRQ_TYPE_NONE>',
        '<&modem_smp2p_in 7 IRQ_TYPE_NONE>',
        '"qcom,wdog"','"qcom,err-fatal"','"qcom,err-ready"',
        '"qcom,proxy-unvote"','"qcom,stop-ack"','"qcom,shutdown-ack"']:
        assert source in modem,source
    assert '<&pdc GIC_SPI 266 IRQ_TYPE_EDGE_RISING>' in modem
    platform=(ROOT/'.cache/microkit-src-2.3.1-sunfish/build/sunfish/debug/sel4/install/support/platform_gen.yaml').read_text()
    device_text=platform.split('devices:\n',1)[1].split('\nmemory:',1)[0]
    device_regions=[(int(start,16),int(end,16)) for end,start in
                    re.findall(r'- end: (0x[0-9a-fA-F]+)\n  start: (0x[0-9a-fA-F]+)',device_text)]
    assert not any(start<=0x17a00000 and 0x17a10000<=end for start,end in device_regions)

    system=ET.parse(PORT/'modem-fatal-trace-probe.system').getroot()
    glink=system.findall("memory_region[@name='glink_prepare_tail']")
    glink_maps=system.findall(".//map[@mr='glink_prepare_tail']")
    assert len(glink)==len(glink_maps)==1
    assert (int(glink[0].get('phys_addr'),0),int(glink[0].get('size'),0),
            glink_maps[0].get('perms'),glink_maps[0].get('cached'))==(0x8611e000,0x4000,'rw','false')
    assert not system.findall("memory_region[@name='gicd_watchdog_status']")
    assert not system.findall(".//map[@mr='gicd_watchdog_status']")
    assert not system.findall('.//irq')
    base_manifest=json.loads((PORT/'loader-task-regions-smp2p.json').read_text())
    trace_manifest=json.loads((PORT/'loader-task-regions-fatal-trace.json').read_text())
    # Fatal-trace capture adds native code and may grow/relocate the AP-copy
    # payload. Its manifest is independently pinned; the final image-layout
    # validator checks it against the newly built ELF and wrapped image.
    assert len(base_manifest['header'])==len(trace_manifest['header'])==8
    assert trace_manifest['header'][0]==base_manifest['header'][0]=='0x5e14dead14de5ead'
    assert len(base_manifest['regions'])==len(trace_manifest['regions'])==11
    assert all(len(row)==4 for row in trace_manifest['regions'])
    assert 'GIC distributor is not mapped' in trace_manifest['scope']
    assert len(trace_manifest['native_pd_elf_sha256'])==64

    source=(ROOT/'rust/src/pixel_modem_status.rs').read_text()
    assert 'gicd' not in source.lower() and 'GICD' not in source
    qrtr=(ROOT/'rust/src/pixel_qrtr_discovery.rs').read_text()
    assert re.search(r'first_fatal_status\s*=\s*value;\s*self\.r\.first_fatal_ms\s*=\s*self\.r\.elapsed_ms',qrtr)
    assert re.search(r'smem::payload\(m,\s*item\)\.ok\(\)',qrtr)
    assert 'first_fatal' in qrtr and 'status_changes' in qrtr
    system_proof=subprocess.check_output([
        'python3','-c',
        'import importlib.util;from pathlib import Path;p=Path("platforms/sunfish/audit-capdl-memory.py");s=importlib.util.spec_from_file_location("a",p);m=importlib.util.module_from_spec(s);s.loader.exec_module(m);print(len(m.fixed_modem_image_pages(Path("platforms/sunfish/modem-fatal-trace-probe.system"),False,True,True,True)))'],
        cwd=ROOT,text=True).strip()
    assert system_proof=='76'
    files=[PORT/'modem-fatal-trace-probe.system',PORT/'loader-task-regions-fatal-trace.json',
           PORT/'audit-capdl-memory.py',PORT/'validate-modem-image-layout.py',
           PORT/'validate-image-boot-layout.py',PORT/'prepare-image-loader-sdk.py',
           PORT/'build-modem-image-probe.sh',PORT/'build-modem-fatal-trace-probe.sh',
           ROOT/'rust/src/pixel_modem_status.rs',ROOT/'rust/src/pixel_qrtr_discovery.rs']
    hashes={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
    print(json.dumps({
        'mode':'read-only SMP2P fatal transition and bounded reason trace',
        'matched_kernel_commit':COMMIT,
        'modem_wdog_gic_spi':266,
        'gicd_mapping':'not mapped; the seL4 platform device map excludes 0x17a00000..0x17a10000',
        'smp2p_signal_bits':{'err_fatal':0,'err_ready':1,'proxy_unvote':2,'stop_ack':3,'shutdown_ack':7},
        'status_item':421,
        'new_gicd_writes':0,'new_irq_caps_or_acknowledgements':0,
        'added_protocol_or_smem_writes':0,'partition_flashed':False,
        'prior_crash_probe_proof':prior,'source_sha256':hashes,
        'interpretation_limit':'SMP2P status transitions and reason text are temporal diagnostic evidence, not proof of root cause. The GIC watchdog pending state is not sampled because the seL4 platform map reserves the distributor range to the kernel.'
    },indent=2))

if __name__=='__main__':main()
