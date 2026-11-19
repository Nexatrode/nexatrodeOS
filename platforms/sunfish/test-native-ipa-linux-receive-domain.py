#!/usr/bin/env python3
"""Full 539-page/eight-table RX geometry and existing SMMU regressions.

All addresses in the tests are host fixtures. No CapDL allocation, table
publication, MMIO, IRQ, DMA or device action is performed by this runner.
"""
from pathlib import Path
import hashlib, json, re, shutil, subprocess
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT/'.cache/sunfish-native-wifi/private/ipa-linux-receive-domain'


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(argv, label):
    p = subprocess.run(argv, cwd=ROOT, capture_output=True, text=True)
    (OUT/(label+'.log')).write_text(p.stdout+p.stderr)
    assert p.returncode == 0, str(OUT/(label+'.log'))
    return p.stdout


def resource_layout():
    base = ET.parse(ROOT/'platforms/sunfish/native-startup-ipa-tables.system').getroot()
    tree = ET.parse(ROOT/'platforms/sunfish/native-startup-ipa-receive.system').getroot()
    pd = tree.find('protection_domain')
    for name, size, address in [('smmu_l3_receive', 0x1000, 0x7100f000),
                                ('ipa_receive_data', 0x200000, 0x76001000)]:
        region = tree.findall(f"memory_region[@name='{name}']")
        assert len(region) == 1 and region[0].attrib == {'name': name, 'size': hex(size)}
        mapping = pd.findall(f"map[@mr='{name}']")
        assert len(mapping) == 1 and mapping[0].attrib == {
            'mr': name, 'vaddr': hex(address), 'perms': 'rw', 'cached': 'false', 'setvar_vaddr': name}
        binding = pd.findall(f"setvar[@symbol='{name}_paddr']")
        assert len(binding) == 1 and binding[0].attrib == {'symbol': name+'_paddr', 'region_paddr': name}
        for guard in [address-4096, address+size]:
            for other in pd.findall('map'):
                mr = tree.find(f"memory_region[@name='{other.get('mr')}']")
                start = int(other.get('vaddr'), 0)
                assert not start <= guard < start+int(mr.get('size'), 0)
        tree.remove(region[0]); pd.remove(mapping[0]); pd.remove(binding[0])
    def shape(node):
        return node.tag, sorted(node.attrib.items()), [shape(n) for n in node]
    assert shape(tree) == shape(base), 'unexpected register/IRQ/resource delta'
    return {'new_private_table_pages': 1, 'new_cpu_receive_pages': 512,
            'additional_mmio_or_irq_authority': False, 'actual_capdl_proven': False}


def main():
    OUT.mkdir(parents=True, exist_ok=True, mode=0o700)
    rustc = shutil.which('rustc')
    assert rustc
    layout = resource_layout()
    groups = {}
    suites = [('domain', 'sunfish_ipa_linux_receive_domain_test.rs', 'rxdomain_'),
              ('old-roots', 'sunfish_smmu_root_tables_test.rs', None),
              ('old-lpae', 'sunfish_smmu_lpae_test.rs', None),
              ('old-native', 'sunfish_smmu_native_test.rs', None)]
    selected = set()
    for label, name, pattern in suites:
        test = ROOT/'tests'/name
        binary = OUT/(label+'-test')
        run([rustc, '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2', '--test', str(test), '-o', str(binary)], label+'-build')
        result = run([str(binary)]+([pattern] if pattern else []), label+'-tests')
        groups[label] = int(re.search(r'test result: ok\. (\d+) passed;', result)[1])
        selected.add(test.relative_to(ROOT).as_posix())
        for path in re.findall(r'#\[path\s*=\s*"([^"]+)"\]', test.read_text()):
            selected.add((test.parent/path).resolve().relative_to(ROOT).as_posix())
    # The shared fixture is a cfg(test) dependency of the native adapter.
    selected.add('tests/sunfish_ipa_shared_fixture.rs')
    names = ['sunfish_ipa_gsi_profile', 'sunfish_ipa_linux_buffers', 'sunfish_ipa_linux_endpoint',
             'sunfish_ipa_linux_gsi', 'sunfish_ipa_linux_receive', 'sunfish_ipa_linux_receive_domain', 'sunfish_smmu_lpae']
    wrapper = OUT/'core.rs'
    # This partial core omits the live register reader. Keep the exact private
    # snapshot-constructor signature referenced without relaxing its warnings
    # or editing the shared source; the function is never called here.
    retain = '''
const _: fn(sunfish_ipa_gsi_profile::ApGsiChannelCapacity, [u8; 10])
    -> Result<sunfish_ipa_gsi_profile::ApGsiChannelSnapshot,
              sunfish_ipa_gsi_profile::ApGsiChannelAllocationError>
    = sunfish_ipa_gsi_profile::ApGsiChannelSnapshot::from_readback;
'''
    wrapper.write_text('#![no_std]\n'+'\n'.join(f'#[path="{ROOT/"rust/src"/(n+".rs")}"] pub mod {n};' for n in names)+retain)
    run([rustc, '--edition=2024', '-D', 'warnings', '--crate-type=lib', '--target=aarch64-unknown-none', str(wrapper), '-o', str(OUT/'receive-domain-aarch64.rlib')], 'aarch64-core')
    selected.update('rust/src/'+n+'.rs' for n in names)
    selected.update(['platforms/sunfish/test-native-ipa-linux-receive-domain.py', 'docs/sunfish-ipa-receive-plan.md',
                     'platforms/sunfish/native-startup-ipa-tables.system', 'platforms/sunfish/native-startup-ipa-receive.system'])
    for rel in sorted(selected):
        p = OUT/'source'/rel
        p.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT/rel, p)
    manifest = {'source_sha256': {rel: sha(ROOT/rel) for rel in sorted(selected)},
        'behavioral_groups': groups, 'mapped_pages': 539, 'receive_pages': 502,
        'cpu_only_tail_pages': 10, 'table_frames': 8, 'root_regions': 2, 'leaf_regions': 4,
        'resource_layout_draft': layout,
        'aarch64_no_std': True, 'hardware_commands': [], 'actual_capdl_allocation': False,
        'installed_receive_mapping': False, 'library_exported_receive_domain': 'pub mod sunfish_ipa_linux_receive_domain;' in (ROOT/'rust/src/lib.rs').read_text(),
        'scope': 'Full receive geometry and pure eight-table image, independent table walks and prior SMMU regression tests; no physical allocation/mapping/DMA proof.'}
    manifest['artifacts'] = {p.name: {'bytes': p.stat().st_size, 'sha256': sha(p)} for p in OUT.iterdir() if p.is_file() and p.name != 'manifest.json'}
    (OUT/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    print(f'PASS: RX domain groups {groups}, 539 mapped pages, eight tables, AArch64 no_std; {OUT}')


if __name__ == '__main__':
    main()
