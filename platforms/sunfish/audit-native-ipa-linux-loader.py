#!/usr/bin/env python3
"""Derive SCM resources and the IPA loader order from the pinned working kernel."""
import argparse
from collections import deque
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile

ROOT=Path(__file__).resolve().parents[2]
PIN="2267e248204e7e051e0f7355298949a6440de579"
ARCHIVE_SHA256="d55c453cea03bc7aeb3381df2538da5423fb926ce6b88dd33eadecbc12a29cb2"


def function(text,name):
    m=re.search(r"^(?:static )?(?:int|void) "+name+r"\([^;]*?\)\s*\{",text,re.M|re.S)
    if not m:raise ValueError("missing reference function "+name)
    depth=1
    for i in range(m.end(),len(text)):
        depth+=(text[i]=="{")-(text[i]=="}")
        if depth==0:return text[m.start():i+1]
    raise ValueError("unclosed function "+name)


def get(dtb,*args):
    return subprocess.check_output(["fdtget",str(dtb),*args],text=True,stderr=subprocess.DEVNULL).strip()

def lease_contract(power):
    """Check the retained owner and both consumers after the shared-helper move."""
    def body(name):
        at=power.index('pub(crate) fn '+name);start=power.index('{',at);depth=1;end=start+1
        while depth:
            depth+=(power[end]=='{')-(power[end]=='}');end+=1
        return ''.join(re.sub(r'//[^\n]*','',power[start:end]).split())
    a,r=body('acquire_lease'),body('release_lease')
    for token in ['self.failure.get().is_some()||self.outstanding.get().is_some()',
                  'self.sequence.get().checked_add(1)', 'self.verify(io,polls,core::hint::spin_loop)?',
                  'self.outstanding.set(Some(sequence))','self.failure.set(Some(e))']:
        if token not in a:raise ValueError('SCM acquire quarantine/identity changed: '+token)
    if a.index('self.verify(')>a.index('self.outstanding.set(Some(sequence))'):raise ValueError('SCM lease before verification')
    for token in ['self.failure.get().is_some()||self.outstanding.get()!=Some(lease.sequence)',
                  'self.outstanding.set(None)','self.verify(io,polls,core::hint::spin_loop)','self.failure.set(Some(e))']:
        if token not in r:raise ValueError('SCM release quarantine/identity changed: '+token)
    if r.index('self.outstanding.set(None)')>r.index('self.verify('):raise ValueError('SCM release not consumed before verification')
    p=''.join(power.split())
    for token in ['self.hold.acquire_lease(self.io,self.polls)','self.hold.release_lease(self.io,self.polls,lease)']:
        if token not in p:raise ValueError('original SCM provider no longer shares identity: '+token)

def lease_mutations(power):
    lease_contract(power)
    cases=[('|| self.outstanding.get().is_some()',''),
           ('self.outstanding.get() != Some(lease.sequence)','false'),
           ('self.outstanding.set(None);','/* omitted consumption */'),
           ('self.verify(io, polls, core::hint::spin_loop)?;','/* omitted acquire verification */'),
           ('self.failure.set(Some(e));','/* omitted persistent quarantine */'),
           ('self.hold.acquire_lease(self.io, self.polls)','Err(Error::Lease)'),
           ('self.hold.release_lease(self.io, self.polls, lease)','Ok(())')]
    for before,after in cases:
        if before not in power:raise ValueError('missing SCM lease mutation: '+before)
        try:lease_contract(power.replace(before,after))
        except ValueError:pass
        else:raise ValueError('SCM lease mutation accepted: '+before)
    return len(cases)


def audit():
    archive=ROOT/".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
    if hashlib.sha256(archive.read_bytes()).hexdigest()!=ARCHIVE_SHA256:raise ValueError("wrong kernel archive")
    paths=["drivers/firmware/qcom/qcom_scm.c","drivers/firmware/qcom/qcom_scm.h",
           "drivers/soc/qcom/mdt_loader.c","drivers/net/ipa/ipa_main.c",
           "drivers/interconnect/qcom/sm7150.c","drivers/interconnect/qcom/icc-rpmh.c",
           "drivers/interconnect/qcom/bcm-voter.c","include/dt-bindings/interconnect/qcom,sm7150-rpmh.h",
           "arch/arm64/boot/dts/qcom/sm7150.dtsi","arch/arm64/boot/dts/qcom/sm7150-google-sunfish.dts"]
    with tarfile.open(archive) as tar:
        source={p:tar.extractfile(f"linux-{PIN}/{p}").read().decode() for p in paths}
    scm=source[paths[0]]
    for name in ["__qcom_scm_pas_init_image","qcom_scm_pas_mem_setup","qcom_scm_pas_auth_and_reset"]:
        body=function(scm,name)
        terms=["qcom_scm_clk_enable()","qcom_scm_bw_enable()","qcom_scm_call(","qcom_scm_bw_disable()","qcom_scm_clk_disable()"]
        positions=[body.index(t) for t in terms]
        if positions!=sorted(positions):raise ValueError("SCM envelope order changed: "+name)
    bw=function(scm,"qcom_scm_bw_enable")
    if "icc_set_bw(__scm->path, 0, UINT_MAX)" not in bw:raise ValueError("SCM bandwidth changed")
    probe=function(scm,"qcom_scm_probe")
    for clock in ["core","iface","bus"]:
        if f'devm_clk_get_optional(&pdev->dev, "{clock}")' not in probe:raise ValueError("SCM clock lookup changed")
    if "QCOM_SCM_ARGS(2, QCOM_SCM_VAL, QCOM_SCM_RW)" not in function(scm,"__qcom_scm_pas_init_image"):
        raise ValueError("metadata pointer ABI changed")
    for name,value in {"QCOM_SCM_SVC_PIL":2,"QCOM_SCM_PIL_PAS_INIT_IMAGE":1,
                       "QCOM_SCM_PIL_PAS_MEM_SETUP":2,"QCOM_SCM_PIL_PAS_AUTH_AND_RESET":5}.items():
        match=re.search(r"#define\s+"+name+r"\s+(0x[0-9a-fA-F]+|\d+)",source[paths[1]])
        if not match or int(match[1],0)!=value:raise ValueError("PAS call ID changed: "+name)
    for token in ["4K aligned and", "non-cachable", "For PIL calls the hypervisor creates SHM Bridges", "not using the TZMem allocator here"]:
        if token not in scm:raise ValueError("metadata allocation contract changed: "+token)
    fw=function(source[paths[3]],"ipa_firmware_load")
    if fw.index("qcom_mdt_load(")>fw.index("qcom_scm_pas_auth_and_reset("):raise ValueError("IPA auth/stage order changed")
    graph=source[paths[4]]
    nodes={name:{"channels":int(re.search(r"\.channels\s*=\s*(\d+)",b)[1]),
                 "width":int(re.search(r"\.buswidth\s*=\s*(\d+)",b)[1]),"links":re.findall(r"&(\w+)",b)}
           for name,b in re.findall(r"static struct qcom_icc_node (\w+) = \{(.*?)\n\};",graph,re.S)}
    queue=deque([("qxm_crypto",["qxm_crypto"])]);seen=set();path=None
    while queue:
        at,route=queue.popleft()
        if at=="ebi":path=route;break
        if at in seen:continue
        seen.add(at);queue.extend((n,route+[n]) for n in nodes[at]["links"])
    if path is None:raise ValueError("missing SCM route")
    bcms={re.search(r'\.name\s*=\s*"(\w+)"',b)[1]:re.findall(r"&(\w+)",b)
          for _,b in re.findall(r"static struct qcom_icc_bcm (\w+) = \{(.*?)\n\};",graph,re.S)}
    selected={b:min(nodes[n]["width"] for n in set(m)&set(path)) for b,m in bcms.items() if set(m)&set(path)}
    if selected!={"MC0":4,"ACV":4,"CE0":8,"SN11":16,"SN0":16,"SH0":16}:raise ValueError("SCM BCM topology changed")
    for token in ["bcm->vote_scale = 1000", ".enable_mask = BIT(3)"]:
        if token not in source[paths[5]]+graph:raise ValueError("BCM scale/ACV mask changed")
    header=source[paths[7]]
    if not re.search(r"#define MASTER_CRYPTO_CORE_0\s+4\b",header) or not re.search(r"#define SLAVE_EBI_CH0\s+1\b",header):
        raise ValueError("SCM binding IDs changed")
    for token in ["[MASTER_CRYPTO_CORE_0] = &qxm_crypto", "[SLAVE_EBI_CH0] = &ebi"]:
        if token not in graph:raise ValueError("SCM provider node changed: "+token)
    if not re.search(r"#define IPA_PAS_ID\s+15\b",source[paths[3]]):raise ValueError("IPA PAS ID changed")
    board=source[paths[9]]
    region=re.search(r"ipa_fw_mem: memory@98f80000 \{(.*?)\};",board,re.S)
    if not region or not re.search(r"reg\s*=\s*<0x0 0x98f80000 0x0 0x10000>",region[1]):
        raise ValueError("IPA reserved target differs")
    dt_records=[]
    for relative in ["artifacts/sm7150-google-sunfish.dtb","device-runs/stage0-v2-ssh/live.dtb"]:
        dtb=ROOT/".cache/sunfish-mobian-comparison"/relative
        props=set(get(dtb,"-p","/firmware/scm").split())
        if props!={"compatible","interconnects"}:raise ValueError("re-audit actual SCM DT properties")
        compatible=get(dtb,"-t","s","/firmware/scm","compatible")
        if compatible!="qcom,scm-sm7150 qcom,scm":raise ValueError("wrong SCM compatible")
        cells=[int(v,16) for v in get(dtb,"-t","x","/firmware/scm","interconnects").split()]
        if len(cells)!=6 or cells[1:3]!=[4,7] or cells[4:]!=[1,7]:raise ValueError("SCM ICC cells changed")
        providers={}
        for child in get(dtb,"-l","/soc").splitlines():
            if not child.startswith("interconnect@"):continue
            node="/soc/"+child
            if "phandle" not in get(dtb,"-p",node).split():continue
            phandle=int(get(dtb,"-t","x",node,"phandle"),16)
            if phandle in [cells[0],cells[3]]:
                if get(dtb,"-t","x",node,"#interconnect-cells")!="2":raise ValueError("provider cell count")
                providers[phandle]=get(dtb,"-t","s",node,"compatible")
        if providers.get(cells[0])!="qcom,sm7150-aggre2-noc" or providers.get(cells[3])!="qcom,sm7150-mc-virt":
            raise ValueError("SCM ICC providers changed")
        dt_records.append({"path":str(dtb),"sha256":hashlib.sha256(dtb.read_bytes()).hexdigest(),"optional_clocks":[],"icc_cells":cells,"providers":providers})
    native_paths=["rust/src/sunfish_ipa_linux_pas.rs","rust/src/sunfish_ipa_linux_loader.rs",
                  "rust/src/sunfish_ipa_linux_loader_native.rs","rust/src/sunfish_scm_linux_power.rs",
                  "rust/src/qcom_rpmh_read.rs","tests/sunfish_ipa_linux_loader_test.rs","tests/sunfish_scm_linux_power_test.rs",
                  "rust/src/qcom_pas_init.rs","rust/src/sunfish_ipa_pas.rs"]
    native=(ROOT/native_paths[1]).read_text();power=(ROOT/native_paths[3]).read_text()
    for token in ["core::mem::ManuallyDrop<M>","self.report.metadata_submitted = true;", "self.report.auth_submitted = true;",
                  "if let Some(error) = staging.error", "io: &mut LinuxTransport<'_, S, R>"]:
        if token not in native:raise ValueError("retained loader contract changed: "+token)
    for token in ["BcmResource::Mc0 => 4", "BcmResource::Ce0 => 8", "_ => 16", "old.vector.x", "old.vector.y | 8", "peak.min(0x3fff)"]:
        if token not in power:raise ValueError("cold SCM arithmetic differs: "+token)
    for token in ["failure: core::cell::Cell<Option<Error>>", "outstanding: core::cell::Cell<Option<u64>>",
                  "self.outstanding.set(None)", "self.failure.set(Some(error))"]:
        if token not in power:raise ValueError("SCM owner quarantine/lease identity changed: "+token)
    lease_cases=lease_mutations(power)
    return {"kernel_commit":PIN,"archive_sha256":ARCHIVE_SHA256,
            "source_sha256":{p:hashlib.sha256(t.encode()).hexdigest() for p,t in source.items()},
            "native_sha256":{p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in native_paths},
            "scm_shared_lease_source_mutations_rejected":lease_cases,
            "scm_route":path,"scm_peak_divisors":selected,"scm_bandwidth_kbps":[0,0xffffffff],
            "dt_records":dt_records,"metadata_bytes":8192,"target":{"physical":"0x98f80000","bytes":65536},
            "intentional_cold_test_differences":["nondecreasing active floors retained until reboot","no attempt to restore unknown AP votes","no suspend/wake vote programming","no busy retry or interrupted-call resumption"],
            "scope":"offline loader/envelope/native memory and SCM hold; no new CapDL/SMC capability, device boot or production readiness"}


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--output",type=Path,required=True);args=parser.parse_args()
    args.output.write_text(json.dumps(audit(),indent=2)+"\n")
    print("Pinned SCM/MDT order, six BCMs, optional clock absence and both actual reference DTs verified.")
