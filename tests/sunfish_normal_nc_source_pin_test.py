import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location('nc_sdk_builder', Path(__file__).resolve().parents[1] / 'platforms/sunfish/build-table-normal-nc-sdk.py')
BUILDER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BUILDER)


class SourcePinTests(unittest.TestCase):
    def test_unlisted_compiled_file_mutation_changes_pin(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'objecttype.c').write_text('original source')
            original = BUILDER.tree_sha(root)
            (root / 'objecttype.c').write_text('altered cache maintenance')
            self.assertNotEqual(original, BUILDER.tree_sha(root))

    def test_added_removed_or_renamed_source_changes_pin(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            a = root / 'a.c'
            a.write_text('code')
            original = BUILDER.tree_sha(root)
            a.rename(root / 'b.c')
            self.assertNotEqual(original, BUILDER.tree_sha(root))
            (root / 'b.c').rename(a)
            self.assertEqual(original, BUILDER.tree_sha(root))
            (root / 'extra.c').write_text('code')
            self.assertNotEqual(original, BUILDER.tree_sha(root))

    def test_build_outputs_do_not_change_source_pin(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'source.c').write_text('code')
            original = BUILDER.tree_sha(root)
            for folder in ['build', 'build-alternate', 'target', 'release', '__pycache__']:
                (root / folder).mkdir()
                (root / folder / 'generated').write_text('output')
            self.assertEqual(original, BUILDER.tree_sha(root))

    def test_nested_build_helpers_are_source_not_outputs(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            folder = root / 'crates/sel4/build-env'
            folder.mkdir(parents=True)
            source = folder / 'lib.rs'
            source.write_text('compiled helper')
            original = BUILDER.tree_sha(root)
            source.write_text('changed compiled helper')
            self.assertNotEqual(original, BUILDER.tree_sha(root))


if __name__ == '__main__':
    unittest.main()
