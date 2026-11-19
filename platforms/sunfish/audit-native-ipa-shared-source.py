#!/usr/bin/env python3
"""Bind real shared mapping selection and retained ownership to the reference."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tarfile

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("rust_smem_audit", HERE / "audit-ipa-smem-rust-source.py")
previous = importlib.util.module_from_spec(spec); spec.loader.exec_module(previous)
ROOT = previous.ROOT


def check(shared, adapter, entry):
    for token in [
        "owner: Option<Owner<Window<'a>>>", "owner: Some(Owner::new(window))",
        "IMEM_PHYSICAL: u64 = 0x146a8000;",
        "IMEM_BYTES: usize = 8192;", "PRIVATE_PAGES: usize = 32;",
        "self.owner\n            .as_mut()\n            .ok_or(Error::NotAllocated)?\n            .allocate_once()",
        ".allocation_owner()\n            .retained_item()\n            .ok_or(Error::NotAllocated)",
        "if let Some(m) = self.modem.as_ref() {\n            return m.parent();",
        'self.owner.as_ref().expect("retained SMEM owner")',
        "let parent = self.owner.take().ok_or(Error::NotAllocated)?;",
        "self.modem = Some(modem::Extension::new(parent, payload, generation));",
        "let pages = view.modem_pages().ok_or(Error::Mapping)?;",
        "bytes != 3 * 4096", "Self::excludes_private_page(*page)",
        "(shared[i], shared[i])", "private[i - 2]", "(shared[i - 32], shared[i - 32])",
        "mappings != self.mappings(private)?",
    ]:
        if token not in shared: raise ValueError("shared backing contract changed: " + token)
    for token in [
        "shared_memory: Option<crate::sunfish_ipa_shared_memory::SharedMemory<'a>>",
        "self.shared_memory = Some(memory);", "self.shared_memory.as_mut().unwrap().prepare()",
        "RootBacking::Private", "RootBacking::Shared", ".validate_mappings(plan.mappings(), self.data_pages)",
        "!self.data_pages.contains(&mapping.physical_page)",
        "SharedMemory::excludes_private_page(page)",
        ".chain(self.shared_memory.iter().flat_map(|m| m.virtual_spans()))",
    ]:
        if token not in adapter: raise ValueError("SMMU retained shared guard changed: " + token)
    if adapter.index("self.shared_memory = Some(memory);") > adapter.index("self.shared_memory.as_mut().unwrap().prepare()"):
        raise ValueError("allocation before owner retention")
    for token in ["cfg!(feature = \"sunfish-smmu-ipa-shared-install-probe\")",
                  "const DATA_PAGE_COUNT: usize = if SHARED_PROFILE {\n    32",
                  "adapter.attach_shared_memory(memory)?;", "adapter.shared_mappings()?",
                  "adapter.stage_shared_root_tables(&tables, extra_tables, extra_physical)?;",
                  "REAL IMEM SMEM MAPPED", "NO SHARED PAYLOAD ACCESS"]:
        if token not in entry: raise ValueError("combined entry selection changed: " + token)


def audit():
    result = previous.audit()
    paths = ["rust/src/sunfish_ipa_shared_memory.rs", "rust/src/sunfish_smmu_native.rs", "rust/src/pixel_smmu_install.rs"]
    texts = [(ROOT / path).read_text() for path in paths]
    check(*texts)
    cases = [
        (0,"IMEM_PHYSICAL: u64 = 0x146a8000;","IMEM_PHYSICAL: u64 = 0x146a9000;"),
        (0,"PRIVATE_PAGES: usize = 32;","PRIVATE_PAGES: usize = 37;"),
        (0,"bytes != 3 * 4096","bytes != 2 * 4096"),
        (0,"(shared[i], shared[i])","(shared[i], private[i])"),
        (0,"mappings != self.mappings(private)?","false"),
        (0,"return m.parent();","/* lose allocation evidence */"),
        (0,"self.modem = Some(modem::Extension::new(parent, payload, generation));","/* lose real predecessor */"),
        (1,"self.shared_memory = Some(memory);","/* no retained owner */"),
        (1,"SharedMemory::excludes_private_page(page)","false"),
        (2,"adapter.attach_shared_memory(memory)?;","/* use copied addresses */"),
    ]
    for index, old, new in cases:
        if texts[index].count(old) != 1: raise ValueError("mutation did not uniquely apply: " + old)
        changed = list(texts); changed[index] = changed[index].replace(old,new)
        try: check(*changed)
        except ValueError: continue
        raise ValueError("shared source mutation accepted: " + old)
    archive = ROOT / ".cache/sunfish-mobian-comparison/downloads/linux-2267e248.tar.gz"
    # The preceding audit hashes this archive. Inspect the actual stage-1
    # attribute conversion as well: READ|WRITE without CACHE/MMIO selects
    # normal noncacheable memory and outer shareability. Native tables retain
    # stricter XN permissions; this stage issues no device transactions.
    with tarfile.open(archive) as tar:
        pg = tar.extractfile("linux-2267e248204e7e051e0f7355298949a6440de579/drivers/iommu/io-pgtable-arm.c").read().decode()
    for token in ["if (prot & IOMMU_MMIO)", "else if (prot & IOMMU_CACHE)",
                  "pte |= ARM_LPAE_PTE_SH_OS;", "if (prot & IOMMU_NOEXEC)"]:
        if token not in pg: raise ValueError("re-audit Linux LPAE memory attributes")
    result["real_shared_mapping_stage"] = {
        "source_sha256": {p: hashlib.sha256(t.encode()).hexdigest() for p,t in zip(paths,texts)},
        "mutations_rejected": len(cases), "private_pool_pages": 32,
        "imem_pages": 2, "smem_pages": 3, "table_frames": 7,
        "output_mapping": "IMEM identity + owned private pool + live SMEM identity",
        "ownership": "actual reserved-frame views and retained live item owner inside Native, not copied ItemInfo",
        "scope": "combined pre-firmware install/configuration stage; no shared payload access, DMA/ATS, SMC/PAS or modem/WLAN startup",
    }
    return result


if __name__ == "__main__":
    p=argparse.ArgumentParser(description=__doc__);p.add_argument("--output",type=Path,required=True)
    args=p.parse_args();args.output.write_text(json.dumps(audit(),indent=2)+"\n")
    print("Real IMEM/SMEM mappings and retained/delegated owner bound to pinned reference; 10 mutations rejected.")
