"""The PAS INIT image adds only one isolated SMC cap to the observed preflight."""
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def shape(node):
    return node.tag, node.attrib, [shape(child) for child in node]


class PasInitProbeTests(unittest.TestCase):
    def test_same_bounded_maps_with_one_smc_owner_and_no_clients(self):
        expected = ET.parse(ROOT / 'platforms/sunfish/modem-prepare-probe.system').getroot()
        pd = expected.find('protection_domain')
        pd.set('name', 'pas_init_probe')
        pd.set('smc', 'true')
        pd.find('program_image').set('path', 'pas_init_probe.elf')
        actual = ET.parse(ROOT / 'platforms/sunfish/pas-init-probe.system').getroot()
        self.assertEqual(shape(actual), shape(expected))


if __name__ == '__main__':
    unittest.main()
