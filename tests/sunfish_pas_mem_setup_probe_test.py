"""No modem-pool AP mapping and only the existing single isolated SMC owner."""
import unittest
import importlib.util
import xml.etree.ElementTree as ET
from pathlib import Path
from tests.sunfish_pas_init_probe_test import shape

ROOT = Path(__file__).resolve().parents[1]


class PasMemSetupProbeTests(unittest.TestCase):
    def test_memory_setup_adds_no_modem_mapping_or_client_authority(self):
        expected = ET.parse(ROOT / 'platforms/sunfish/pas-init-probe.system').getroot()
        pd = expected.find('protection_domain')
        pd.set('name', 'pas_mem_setup_probe')
        pd.find('program_image').set('path', 'pas_mem_setup_probe.elf')
        actual = ET.parse(ROOT / 'platforms/sunfish/pas-mem-setup-probe.system').getroot()
        self.assertEqual(shape(actual), shape(expected))

    def test_wrong_pas_pool_or_any_intersecting_ap_frame_refuses_build(self):
        spec = importlib.util.spec_from_file_location('mem_layout', ROOT / 'platforms/sunfish/validate-pas-mem-setup-layout.py')
        layout = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(layout)
        pool = [('modem_region@8b000000', 0x8b000000, 0x9800000, True)]
        pages = [(0x80000000, 4096, 'metadata'), (0x94800000, 4096, 'at_upper_boundary')]
        self.assertEqual(layout.validate(pages, pool, pool, [4], [0xb6], [0xb6])['ap_frames_in_modem_reservation'], 0)
        for args in [
            (pages, pool, pool, [5], [0xb6], [0xb6]),
            (pages, pool, pool, [4], [0xb7], [0xb6]),
            (pages, pool, pool, [4], [], []),
            (pages, pool, pool + pool, [4], [0xb6], [0xb6]),
            ([], pool, pool, [4], [0xb6], [0xb6]),
        ]:
            with self.assertRaises(ValueError): layout.validate(*args)
        for bad_pool in [[], [('modem_region@8b000000', 0x8b000000, 0x9800000, False)],
                         [('modem_region@8b000000', 0x8b000000, 0x9700000, True)],
                         pool + [('other', 0x8b001000, 4096, True)]]:
            for kernel, live in [(bad_pool, pool), (pool, bad_pool)]:
                with self.assertRaises(ValueError): layout.validate(pages, kernel, live, [4], [0xb6], [0xb6])
        for start in [0x8afff000, 0x8b000000, 0x8b001000, 0x947ff000]:
            with self.assertRaises(ValueError):
                layout.validate(pages + [(start, 8192, 'bad')], pool, pool, [4], [0xb6], [0xb6])


if __name__ == '__main__': unittest.main()
