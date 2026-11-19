"""Exact whole-image mapping, dedicated snapshot RAM, no added client authority."""
import copy
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from tests.sunfish_modem_data_probe_test import PORT, load

class ModemImageTests(unittest.TestCase):
    def test_whole_pool_opt_in_is_exact_and_keeps_nx_and_private_snapshot(self):
        audit = load('audit-capdl-memory')
        self.assertEqual(audit.fixed_modem_image_pages(None), set())
        original = ET.parse(PORT / 'modem-image-probe.system').getroot()
        self.assertEqual(len(audit.fixed_modem_image_pages(PORT / 'modem-image-probe.system')), 76)
        changes = [("memory_region[@name='modem_image_pool']", 'phys_addr', '0x8b200000'),
                   ("memory_region[@name='modem_image_pool']", 'size', '0x9a00000'),
                   ("memory_region[@name='modem_image_pool']", 'page_size', '0x1000'),
                   (".//map[@mr='modem_image_pool']", 'perms', 'rwx'),
                   (".//map[@mr='modem_image_pool']", 'cached', 'true'),
                   ("memory_region[@name='modem_gap_snapshot']", 'phys_addr', '0x80000000'),
                   ("memory_region[@name='modem_gap_snapshot']", 'size', '0x400000'),
                   (".//map[@mr='modem_gap_snapshot']", 'perms', 'rwx')]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'changed.system'
            for selector, attr, value in changes:
                changed = copy.deepcopy(original)
                changed.find(selector).set(attr, value)
                ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError): audit.fixed_modem_image_pages(path)
            for extra in [ET.Element('channel'), ET.Element('protection_domain', name='client')]:
                changed = copy.deepcopy(original)
                changed.append(extra)
                ET.ElementTree(changed).write(path)
                with self.assertRaises(ValueError): audit.fixed_modem_image_pages(path)
            changed = copy.deepcopy(original)
            pd = changed.find('protection_domain')
            pd.append(copy.deepcopy(pd.find("map[@mr='modem_image_pool']")))
            ET.ElementTree(changed).write(path)
            with self.assertRaises(ValueError): audit.fixed_modem_image_pages(path)

    def test_all_pool_pages_required_and_any_extra_alias_or_frame_rejected(self):
        layout = load('validate-modem-image-layout')
        pool = [('modem_region@8b000000', 0x8b000000, 0x9800000, True)]
        pages = [(start, 0x200000, 'frame_mr_modem_image_pool_' + str(i))
                 for i, start in enumerate(range(0x8b000000,0x94800000,0x200000))]
        snapshot = (0x80000000,0x200000,'frame_mr_modem_gap_snapshot_0')
        def validate(p):
            return layout.validate(p,pool,pool,[4],[1],[1],PORT / 'modem-image-probe.system')
        self.assertEqual(validate(pages + [snapshot])['ap_frames_in_modem_reservation'],76)
        for p in [pages[1:]+[snapshot], pages+[pages[0],snapshot],
                  pages+[snapshot,(0x8b000000,4096,'alias')],
                  pages+[snapshot,(0x947ff000,4096,'alias')],
                  pages[:-1]+[(0x94600000,0x400000,pages[-1][2]),snapshot]]:
            with self.assertRaises(ValueError): validate(p)

    def test_gap_snapshot_cannot_alias_pool_other_frames_or_fixed_reservations(self):
        layout = load('validate-modem-image-layout')
        pool = [('modem_region@8b000000',0x8b000000,0x9800000,True)]
        pages = [(start,0x200000,'frame_mr_modem_image_pool_'+str(i))
                 for i,start in enumerate(range(0x8b000000,0x94800000,0x200000))]
        def validate(p,regions=pool):
            return layout.validate(p,regions,regions,[4],[1],[1],PORT / 'modem-image-probe.system')
        name = 'frame_mr_modem_gap_snapshot_0'
        for extra in [[],[(0x8b000000,0x200000,name)],[(0x80001000,0x200000,name)],
                      [(0x80000000,4096,name)],[(0x80000000,0x200000,name)]*2,
                      [(0x80000000,0x200000,name),(0x80001000,4096,'ELF_alias')]]:
            with self.assertRaises(ValueError): validate(pages+extra)
        with self.assertRaises(ValueError):
            validate(pages+[(0x80000000,0x200000,name)],pool+[('other_reserved',0x801ff000,4096,True)])

    def test_servreg_state_profile_scope_stays_read_only_and_distinct_from_restart(self):
        layout = load('validate-modem-image-layout')
        pool = [('modem_region@8b000000', 0x8b000000, 0x9800000, True)]
        pages = [(start, 0x200000, 'frame_mr_modem_image_pool_' + str(i))
                 for i, start in enumerate(range(0x8b000000, 0x94800000, 0x200000))]
        snapshot = (0x80000000, 0x200000, 'frame_mr_modem_gap_snapshot_0')
        system = PORT / 'modem-servreg-state-probe.system'
        wrapper = (PORT / 'build-modem-servreg-state-probe.sh').read_text()
        self.assertIn('export SUNFISH_MODEM_SERVREG_RESTART_PROBE=0', wrapper)

        state = layout.validate(pages + [snapshot], pool, pool, [4], [1], [1], system,
                                handshake=True, servreg_state_probe=True)
        self.assertIn('state query only', state['scope'])
        self.assertIn('no listener registration', state['scope'])
        self.assertIn('no PDR restart', state['scope'])
        self.assertIn('no WLAN power', state['scope'])

        restart = layout.validate(pages + [snapshot], pool, pool, [4], [1], [1], system,
                                  handshake=True, servreg_state_probe=True,
                                  servreg_restart_probe=True)
        self.assertIn('one guarded PDR restart', restart['scope'])
        self.assertIn('no WLAN power', restart['scope'])

if __name__ == '__main__': unittest.main()
