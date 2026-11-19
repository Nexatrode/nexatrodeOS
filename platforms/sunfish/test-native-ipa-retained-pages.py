#!/usr/bin/env python3
"""Exercise retained RX table checks against eight independently corrupted pages.

The temporary copy only injects a test module into the unchanged native source;
all register and table accesses use the existing host RAM fixture.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = ROOT / 'rust/src/sunfish_smmu_native.rs'
    fixture = ROOT / 'tests/sunfish_ipa_shared_fixture.rs'
    cases = ROOT / 'tests/sunfish_ipa_retained_pages_cases.rs'
    native = source.read_text()
    old_fixture = '#[path = "../../tests/sunfish_ipa_shared_fixture.rs"]'
    assert native.count(old_fixture) == 1
    native = native.replace(old_fixture, f'#[path = "{fixture}"]')
    marker = '#[cfg(test)]\nmod tests {'
    assert native.count(marker) == 1
    native = native.replace(marker, marker + f'\n    include!("{cases}");', 1)

    with tempfile.TemporaryDirectory(prefix='sunfish-retained-pages-') as directory:
        temp = Path(directory)
        native_copy = temp / 'sunfish_smmu_native.rs'
        native_copy.write_text(native)
        harness = (ROOT / 'tests/sunfish_smmu_native_test.rs').read_text()
        harness = harness.replace('../rust/src/', str(ROOT / 'rust/src') + '/')
        original = str(ROOT / 'rust/src/sunfish_smmu_native.rs')
        assert harness.count(original) == 1
        harness = harness.replace(original, str(native_copy))
        test_file = temp / 'harness.rs'
        test_file.write_text(harness)
        binary = temp / 'native-test'
        subprocess.run(['rustc', '--edition=2024', '-D', 'warnings', '-C', 'opt-level=2',
                        '--cfg', 'feature="sunfish-smmu-install-probe"', '--test',
                        str(test_file), '-o', str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary),
                        'retained_receive_reports_each_page_and_rejects_corruption_in_all_eight'],
                       cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
