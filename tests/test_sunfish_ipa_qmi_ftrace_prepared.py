"""Offline checks of the prepared, isolated Linux reference trace."""
from pathlib import Path
import hashlib
import json
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / '.cache/sunfish-native-wifi/private/ipa-qmi-ftrace-prepared-v2'

class PreparedTrace(unittest.TestCase):
    def setUp(self):
        self.script = OUT / 'start-coldwifi-ipa-ftrace.sh'
        self.text = self.script.read_text()
        self.manifest = json.loads((OUT / 'manifest.json').read_text())
        self.begin = self.text.index('\n# A private tracefs instance')
        end = self.text.index('printf \'Scoped IPA function-entry tracing active before modem startup\\n\' > "$work/logs/ipa-ftrace-setup.txt"', self.begin)
        self.end = self.text.index('\n', end) + 1
        self.setup = self.text[self.begin:self.end]

    def test_only_the_scoped_trace_is_added_to_the_pinned_reference(self):
        base = (ROOT / 'platforms/sunfish/mobian-comparison/start-coldwifi.sh').read_text()
        self.assertEqual(self.text[:self.begin] + self.text[self.end:], base)
        self.assertEqual(hashlib.sha256(self.script.read_bytes()).hexdigest(), self.manifest['capture_script_sha256'])
        self.assertLess(self.text.index('test "$(cat "$rp/state")" = offline'), self.begin)
        self.assertLess(self.end, self.text.index('sh /usr/share/sunfish-reference/wcn/sbin/wcn-wifi-bringup.sh'))
        subprocess.run(['sh', '-n', str(self.script)], check=True)

    def test_filter_is_bounded_and_cleanup_preserves_result(self):
        self.assertIn('trace_instance=$trace_root/instances/sunfish_ipa_ref', self.setup)
        self.assertIn('test ! -e "$trace_instance"', self.setup)
        self.assertIn('count != 7', self.setup)
        self.assertLess(self.setup.index('trap capture_ipa_trace EXIT'), self.setup.index('printf \'1\\n\' > "$trace_instance/tracing_on"'))
        self.assertIn('per_cpu/cpu*/stats', self.setup)
        self.assertIn('rmdir "$trace_instance"', self.setup)
        self.assertIn('exit "$status"', self.setup)
        for fn in self.manifest['functions']:
            self.assertIn(fn, self.setup)
        self.assertNotIn('wpa_supplicant', self.setup)

if __name__ == '__main__':
    unittest.main()
