#!/usr/bin/env python3
"""Bind the actual native IPA firmware entry and post-auth guard to Linux."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tarfile

HERE=Path(__file__).resolve().parent
def module(name,file):
    spec=importlib.util.spec_from_file_location(name,HERE/file);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
loader=module("native_firmware_loader_audit","audit-native-ipa-linux-loader.py")
uc=module("native_firmware_uc_audit","audit-native-ipa-uc-source.py")
ROOT=loader.ROOT


def check(entry,parent,config,secure):
    for token in ["state._firmware_rsc = Some(controller)","loader::Image::private_reference(FIRMWARE)",
                  "state._firmware = match NativeLoader::new", "0x75601000", "0x75800000",
                  "physical.checked_add(4096)", "&raw const ipa_pas_metadata_paddr", "&raw const ipa_fw_target_paddr",
                  "metadata_named(dict, db, scm::NAMES)", "state._scm_hold.enable(",
                  "HeldResources::new(", "LinuxTransport::new(&mut secure, &mut resources)",
                  "guard(domain, pool, registers, expected)", "read_retained_post_auth_snapshot(",
                  "sunfish_ipa_linux_irq::ALLOWED_DISABLED_STATUS"]:
        if token not in entry:raise ValueError("native firmware dependency changed: "+token)
    if entry.index("state._firmware = match NativeLoader::new")>entry.index(".prepare()"):
        raise ValueError("metadata touched before owner retained")
    if entry.index("state._scm_hold.enable(")>entry.index(".prepare()"):
        raise ValueError("SCM hold not established before metadata/PAS")
    for token in ["pixel_ipa_firmware::start(","firmware: Option<&'a pixel_ipa_firmware::NativeLoader>",
                  "!loader.pas_report().passed()", "self.retained_prerequisites()", "config::verify_retained_after_auth("]:
        if token not in parent:raise ValueError("native post-auth retained-owner guard changed: "+token)
    for token in ["pub fn verify_retained_after_auth(","validate(cold)?", "cold.guards[3] = 12", "cold.guards[4] = 1", "if capture(io)? != cold"]:
        if token not in config:raise ValueError("post-auth status/configuration check changed: "+token)
    for token in ["let r = request.words()", "seL4_ARM_SMC_Call(7, &args, &mut reply)", 'asm!("dsb sy"']:
        if token not in secure:raise ValueError("sealed SMC transport changed: "+token)


def audit():
    result=loader.audit();result["pre_firmware_prerequisites"]=uc.audit()
    paths=["rust/src/pixel_ipa_firmware.rs","rust/src/sunfish_native_startup_preflight.rs",
           "rust/src/sunfish_ipa_linux_config.rs","rust/src/pixel_pas_smc.rs"]
    texts=[(ROOT/p).read_text() for p in paths];check(*texts)
    cases=[(0,"0x75601000","0x75602000"),(0,"0x75800000","0x75801000"),
           (0,"state._firmware = match NativeLoader::new","let _unretained = match NativeLoader::new"),
           (0,"LinuxTransport::new(&mut secure, &mut resources)","legacy_transport"),
           (1,"!loader.pas_report().passed()","false"),(2,"cold.guards[4] = 1","cold.guards[4] = 0"),
           (3,"seL4_ARM_SMC_Call(7, &args, &mut reply)","seL4_ARM_SMC_Call(8, &args, &mut reply)")]
    for index,old,new in cases:
        if texts[index].count(old)!=1:raise ValueError("mutation did not uniquely apply: "+old)
        changed=texts.copy();changed[index]=changed[index].replace(old,new)
        try:check(*changed)
        except ValueError:continue
        raise ValueError("native firmware source mutation accepted: "+old)
    with tarfile.open(ROOT/".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz") as tar:
        refs={p:tar.extractfile(f"linux-{loader.PIN}/{p}").read().decode() for p in ["drivers/net/ipa/gsi.c","drivers/net/ipa/reg/gsi_reg-v4.0.c"]}
    body=loader.function(refs["drivers/net/ipa/gsi.c"],"gsi_setup")
    if 'if (!(val & reg_bit(reg, ENABLED)))' not in body:raise ValueError("GSI post-auth requirement changed")
    if '[ENABLED]\t\t\t\t\t= BIT(0)' not in refs["drivers/net/ipa/reg/gsi_reg-v4.0.c"]:raise ValueError("GSI enabled bit changed")
    result["native_firmware_entry"]={"source_sha256":{p:hashlib.sha256(t.encode()).hexdigest() for p,t in zip(paths,texts)},
        "reference_sha256":{p:hashlib.sha256(t.encode()).hexdigest() for p,t in refs.items()},
        "source_mutations_rejected":len(cases),"post_auth_gsi_status":1,"post_auth_uc_enable":12,
        "scope":"One native PAS15 start; retained ownership/guards; no host GSI/DMA or modem/WLAN start",
        "device_result":"pending; source/host checks alone do not prove PAS or IRQ delivery"}
    return result


if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__);p.add_argument("--output",type=Path,required=True);args=p.parse_args()
    args.output.write_text(json.dumps(audit(),indent=2)+"\n")
    print("Native IPA PAS15 entry, retained owners, sealed SMC and strict post-auth guard source-bound; 7 mutations rejected.")
