"""Scope checks for the active-only Linux reference snapshot helper."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / 'platforms/sunfish/mobian-comparison/ipa-config-readback/sunfish_ipa_config_readback.c').read_text()


class ReadbackScope(unittest.TestCase):
    def test_only_two_fixed_registers_are_read_twice(self):
        self.assertIn('#define CLKON_CFG 0x44', SOURCE)
        self.assertIn('#define QSB_MAX_WRITES 0x74', SOURCE)
        self.assertEqual(SOURCE.count('readl('), 4)
        self.assertEqual(SOURCE.count('readl(base + CLKON_CFG)'), 2)
        self.assertEqual(SOURCE.count('readl(base + QSB_MAX_WRITES)'), 2)
        for forbidden in ('writel(', 'writeb(', 'writew(', 'pm_runtime_get_sync(',
                          'pm_runtime_resume_and_get(', 'device_release_driver('):
            self.assertNotIn(forbidden, SOURCE)

    def test_active_owned_sunfish_resource_is_required_before_reading(self):
        for requirement in ('"google,sunfish"', '"qcom,sm7150-ipa"',
                            'resource.start != EXPECTED_BASE',
                            'resource_size(&resource) != EXPECTED_SIZE',
                            '!pdev->dev.driver', '!pm_runtime_enabled(&pdev->dev)'):
            self.assertIn(requirement, SOURCE)
        self.assertLess(SOURCE.index('pm_runtime_get_if_active('), SOURCE.index('ioremap('))
        self.assertLess(SOURCE.index('if (ret <= 0)'), SOURCE.index('ioremap('))
        self.assertLess(SOURCE.index('iounmap(base)'), SOURCE.index('pm_runtime_put('))


if __name__ == '__main__':
    unittest.main()
