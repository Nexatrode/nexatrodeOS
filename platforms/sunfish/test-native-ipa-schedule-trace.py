#!/usr/bin/env python3
"""Check IPA scheduling observations using the selected runtime feature profile.

Offline only. Pass the predecessor's features.txt; the candidate adds the
schedule observer. Logs, compiler arguments and source identities are retained.
"""
from pathlib import Path
import argparse
import json
import os
import subprocess

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--features', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True, mode=0o700)
baseline = args.features.read_text().strip().split(',')
assert 'sunfish-ipa-schedule-trace' not in baseline
(out / 'features-baseline.txt').write_text(','.join(baseline) + '\n')
(out / 'features.txt').write_text(','.join(baseline + ['sunfish-ipa-schedule-trace']) + '\n')
env = dict(os.environ, RUST_MIN_STACK='16777216', RUSTUP_TOOLCHAIN='nightly-2026-09-25')

def run(command, label, extra_env=None):
    result = subprocess.run(command, cwd=ROOT, env=extra_env or env, capture_output=True, text=True)
    (out / (label + '-command.json')).write_text(json.dumps(command, indent=2) + '\n')
    (out / (label + '.log')).write_text(result.stdout + result.stderr)
    if result.returncode:
        print((result.stdout + result.stderr)[-6000:])
        raise SystemExit(f'{label} failed; see {out}')
    print(label + ': passed', flush=True)

for name, features in [('host-on', 'features.txt'), ('host-off', 'features-baseline.txt')]:
    run(['python3', 'platforms/sunfish/test-native-wlan-state-query.py',
         '--features', str(out / features), '--output', str(out / name)], name)
features = json.loads((out / 'host-on/result.json').read_text())['features']
flags = ['rustc', '--edition=2024', '--test', '-D', 'warnings', '-A', 'dead_code',
         '-A', 'deprecated', '-C', 'opt-level=2']
for feature in features:
    flags += ['--cfg', f'feature="{feature}"']
link_env = dict(env, SUNFISH_LINUX_QMI_LINK_C=str(
    ROOT / '.cache/sunfish-native-wifi/private/ipa-linux-qmi-link/linux-wire.txt'))
run(flags + ['tests/sunfish_ipa_linux_qmi_link_test.rs', '-o', str(out / 'link-tests')], 'link-build', link_env)
run([str(out / 'link-tests'), 'linuxlink_init_', '--test-threads=1'], 'link-tests', link_env)
run(flags + ['tests/pixel_mpss_runtime_diagnostic_test.rs', '-o', str(out / 'timing-tests')], 'timing-build')
run([str(out / 'timing-tests')], 'timing-tests')
run(['python3', 'platforms/sunfish/test-native-mpss-runtime-source.py'], 'runtime-source-tests')
# The final row is reserved for the existing footer. Each value is 8 hex digits.
source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
panel = source.split('fn show_ipa_schedule(', 1)[1].split('fn show_ipa_init_tx(', 1)[0]
import re
labels = re.findall(r'\("([A-Z0-9 ]+)",', panel)
# Use the actual renderer budget: 24 columns minus a space and 8 hex digits.
renderer = (ROOT / 'rust/src/pixel_probe_display.rs').read_text().split('pub fn hex(', 1)[1]
columns = int(re.search(r"let mut line = \[b' '; (\d+)\]", renderer)[1])
assert 'label.len() > line.len() - 9' in renderer
assert len(labels) == 27 and all(len(label) + 9 <= columns for label in labels)
assert labels[-1] == 'WLFW SERVERS'
print('PASS: both runtime profiles, checked link arbitration, timing boundaries, source checks and panel bounds; no device action.')
