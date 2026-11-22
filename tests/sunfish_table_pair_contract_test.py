"""Pin the public mapping obligation and feature gate for wide table reads."""
from pathlib import Path
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]

class TablePairContract(unittest.TestCase):
    def test_public_staging_contract_covers_all_eight_normal_nc_frames(self):
        source = (ROOT/'rust/src/sunfish_smmu_native.rs').read_text()
        end = source.index('pub unsafe fn stage_receive_root_tables(')
        contract = source[source.rfind('/// # Safety', 0, end):end]
        for phrase in ['all eight', "Native::new's four", 'Normal-NC', 'no concurrent CPU/device writer', 'throughout retention']:
            self.assertIn(phrase, contract)

    def test_pair_feature_requires_the_normal_nc_and_previous_safe_profiles(self):
        graph = tomllib.loads((ROOT/'rust/Cargo.toml').read_text())['features']
        todo = ['sunfish-smmu-normal-nc-table-pairs']
        seen = set()
        while todo:
            feature = todo.pop()
            if feature in seen:
                continue
            seen.add(feature)
            todo.extend(graph.get(feature, []))
        self.assertTrue({'sunfish-wrq-table-normal-nc', 'sunfish-runtime-handoff-stack-split', 'sunfish-tftp-peer-metadata-batch'} <= seen)

if __name__ == '__main__':
    unittest.main()
