#!/usr/bin/env python3
"""Pin observed fatal flag, live-DTS reason item and read-only crash capture."""
import json,subprocess,hashlib,re
from pathlib import Path
PORT=Path(__file__).resolve().parent
ROOT=PORT.parents[1]
def main():
    proof=json.loads(subprocess.check_output(['python3',str(PORT/'validate-qrtr-wait-inputs.py')],text=True))
    prior=json.loads((ROOT/'.cache/pixel-sunfish/modem-qrtr-wait-probe/native-observation.json').read_text())
    assert prior['artifact_sha256']=='064949cba5f45aac1a2b4f2b91174e233d7c4d5bafd6a734c7e4e8dbabad8cab'
    assert prior['error']=='0x0e' and prior['error_name_from_frozen_program']=='Fatal'
    assert prior['last_poll_ms']==38663 and prior['clock_hz']==19200000 and prior['data_polls']==1440
    assert prior['old_guards'] and prior['pool_guards'] and not prior['last_slave_kernel_shown']
    refs={}
    for n in ['.cache/pixel-sunfish/live-lineage-20261010.dts','.cache/sunfish-lineage-kernel/drivers/soc/qcom/smem.c','.cache/sunfish-lineage-kernel/drivers/soc/qcom/pil-q6v5-mss.c']:
        p=ROOT/n;refs[n]=hashlib.sha256(p.read_bytes()).hexdigest()
    dt=(ROOT/'.cache/pixel-sunfish/live-lineage-20261010.dts').read_text()
    modem=dt[dt.index('qcom,mss@4080000'):];modem=modem[:modem.index('\n\t\t};')]
    assert 'qcom,smem-id = <0x1a5>;' in modem and 'qcom,pas-id = <0x04>;' in modem
    git_ref='drivers/soc/qcom/subsys-pil-tz.c'
    driver_bytes=subprocess.check_output(['git','-C',str(ROOT/'.cache/sunfish-lineage-kernel'),'show','28f9290ae067d3e7857126eb6312720dbe9c7c80:'+git_ref])
    driver=driver_bytes.decode()
    assert 'qcom,pil-tz-generic' in driver and 'qcom,pil-tz-generic' in modem
    assert 'qcom_smem_get(QCOM_SMEM_HOST_ANY, d->smem_id, &size)' in driver
    assert 'MAX_SSR_REASON_LEN' in driver
    parser=(ROOT/'rust/src/qcom_modem_crash.rs').read_text();native=(ROOT/'rust/src/pixel_modem_crash.rs').read_text()
    assert 'pub const ITEM_ID: u16 = 421;' in parser and re.search(r'header_hosts\.0\s*!=\s*0xfffe\s*&&\s*header_hosts\.1\s*!=\s*0xfffe',parser) and 'pub fn partition_stage' in parser
    assert 'Other host-pair entries describe separate heaps' in parser and 'all reads remain bounded by the selected global route' in parser
    assert all(x not in parser+native for x in ['write_volatile','*mut','asm!','Transport','Smc'])
    entry=(ROOT/'rust/src/sunfish_modem_smp2p_probe.rs').read_text()
    baseline=re.search(r'let\s+crash_baseline\s*=\s*pixel_modem_crash::capture\(&mut observer\)',entry)
    assert baseline and baseline.start() < entry.index('let mut smc = Smc;')
    crash_after=re.search(r'let\s+crash_after\s*=',entry)
    crash_status=re.search(r'let\s+crash_status\s*=',entry)
    assert entry.index('core::mem::forget(discovery);') < crash_after.start() < crash_status.start()
    assert '"LAST SLAVE KERNEL"' not in entry
    proof.update(previous_native_measured_fatal=prior,reason_item=421,global_hosts=[65534,65534],reason_max_bytes=256,
        reason_item_size_limit=4096,reason_snapshot_attempts=3,baseline_before_secure_call=True,baseline_difference_diagnostic_only=True,
        reason_clear=False,post_terminal_protocol_action=False,extra_packet_or_write_authority=False,
        reference_sha256=refs,git_reference_sha256={git_ref:hashlib.sha256(driver_bytes).hexdigest()},matched_kernel_base_commit='28f9290ae067d3e7857126eb6312720dbe9c7c80',
        source_trace_not_running_binary_provenance=True,
        scope='Repeat measured-wait transport policy and all ownership/write bounds. Before any secure call capture global v12 item421, then after terminal discovery capture it again and sample raw SMP2P status read-only. Select exactly one65534/65534 route; validate only its bounded global partition header and allocation metadata. Ignore unrelated host-pair ranges because they are not followed by the global-reason reader. Perform duplicate rejection, metadata revalidation and two equal256byte-prefix reads; at most3 instability attempts. Preserve raw bytes privately, show sanitized text and compare bounded prefixes to flag stale content. No reason clear, protocol action after terminal result, buffer release, QMI, DMA, reset or retry. Reason text is remote diagnostic evidence, never a command or proof of root cause.')
    print(json.dumps(proof,indent=2))
if __name__=='__main__':main()
