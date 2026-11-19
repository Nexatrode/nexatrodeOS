#!/usr/bin/env python3
"""Validate the composed independent IPA profile; no device actions.

Runs both full runtime profiles, the paired core/owner/link comparison, and
renders every diagnostic row with the actual framebuffer renderer on the host.
AArch64 boot-image build and loader/capability audits remain separate steps.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
FEATURE = 'sunfish-ipa-independent-server-runtime'
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--baseline-features', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
p.add_argument('--reuse-verified-results', action='store_true', help='Reuse only passed results whose recorded sources still match.')
a = p.parse_args()
out = a.output.resolve()
out.mkdir(parents=True, exist_ok=True, mode=0o700)
env = dict(os.environ, RUSTUP_TOOLCHAIN='nightly-2026-09-25', RUST_MIN_STACK='16777216')
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()

def run(command, label):
    (out / (label + '-command.json')).write_text(json.dumps(command, indent=2) + '\n')
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True)
    (out / (label + '.log')).write_text(result.stdout + result.stderr)
    if result.returncode:
        print((result.stdout + result.stderr)[-6000:])
        raise SystemExit(label + ' failed')
    print(label + ': passed', flush=True)


def verified(path):
    result = json.loads(path.read_text())
    assert result['result'] == 'passed' and result['device_actions'] == []
    for name, digest in result['source_sha256'].items():
        assert sha(ROOT / name) == digest, name
    for item in result.get('fixture_sha256', {}).values():
        assert sha(Path(item['path'])) == item['sha256']
    return result


def closure(features):
    graph = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())['features']
    selected = set(features)
    pending = list(selected)
    while pending:
        for name in graph.get(pending.pop(), []):
            if name in graph and name not in selected:
                selected.add(name)
                pending.append(name)
    return sorted(selected)

baseline = a.baseline_features.read_text().strip().split(',')
assert 'sunfish-ipa-schedule-trace' in baseline and FEATURE not in baseline
candidate = [f for f in baseline if f != 'sunfish-ipa-schedule-trace'] + [FEATURE]
profiles = {}
for mode, features in [('baseline', baseline), ('candidate', candidate)]:
    file = out / ('features-baseline.txt' if mode == 'baseline' else 'features.txt')
    file.write_text(','.join(features) + '\n')
    directory = out / ('host-' + mode)
    if not a.reuse_verified_results:
        run(['python3', str(ROOT / 'platforms/sunfish/test-native-wlan-state-query.py'),
             '--features', str(file), '--output', str(directory)], 'host-' + mode)
    result = verified(directory / 'result.json')
    assert result['features'] == closure(features)
    profiles[mode] = dict(passed=result['passed'], ignored=result['ignored'],
                          result_sha256=sha(directory / 'result.json'))
if not a.reuse_verified_results:
    run(['python3', str(ROOT / 'platforms/sunfish/test-native-ipa-independent-server.py'),
         '--output', str(out / 'offline-comparison')], 'paired-comparison')
verified(out / 'offline-comparison/result.json')
run(['python3', str(ROOT / 'platforms/sunfish/test-native-mpss-runtime-source.py')], 'runtime-source-tests')
source = ROOT / 'rust/src/pixel_mpss_runtime.rs'
text = source.read_text()
panel = text.split('fn show_ipa_independent(', 1)[1].split('fn show_ipa_schedule(', 1)[0]
labels = re.findall(r'\("([A-Z0-9 ]+)",', panel)
renderer = ROOT / 'rust/src/pixel_probe_display.rs'
render_text = renderer.read_text().split('pub fn hex(', 1)[1]
columns = int(re.search(r"let mut line = \[b' '; (\d+)\]", render_text)[1])
assert 'label.len() > line.len() - 9' in render_text
assert len(labels) == 27 and labels[-1] == 'WLFW SERVERS'
assert all(len(label) + 9 <= columns for label in labels)
assert 'ipa_schedule.phase_ms' not in panel
assert 'not(feature = "sunfish-ipa-independent-server-runtime")' in text
assert 'independent IPA runtime uses overlapping event timestamps' in text
# Compile the actual feature guards in isolation, including both rejected
# native combinations. The complete permitted binary is built separately.
start = text.index('#[cfg(all(feature = "sunfish-ipa-independent-server-experiment"')
end = text.index('const GENERATION:', start)
guards = out / 'native-feature-guards.rs'
guards.write_text(text[start:end] + '\nfn main() {}\n')
for name, selected, expected in [
    ('bare-experiment', ['sunfish-ipa-independent-server-experiment'], 'offline only'),
    ('composed-profile', ['sunfish-ipa-independent-server-experiment', FEATURE], None),
    ('serialized-observer', ['sunfish-ipa-independent-server-experiment', FEATURE, 'sunfish-ipa-schedule-trace'], 'overlapping event timestamps'),
]:
    command = ['rustc', '--edition=2024', '--emit=metadata', str(guards), '-o', str(out / (name + '.rmeta'))]
    for feature in selected:
        command += ['--cfg', f'feature="{feature}"']
    result = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True)
    (out / (name + '-guard.log')).write_text(result.stdout + result.stderr)
    (out / (name + '-guard-command.json')).write_text(json.dumps(command, indent=2) + '\n')
    assert (result.returncode == 0) == (expected is None), name
    if expected is not None:
        assert expected in result.stderr, name
print('native feature guards: passed', flush=True)
# The real renderer must paint every row in both buffers, including each
# label/value boundary. No synthetic framebuffer is used as device evidence.
harness = out / 'panel-render.rs'
harness.write_text(f'#[path={json.dumps(str(renderer))}] mod display;\n' + r'''
fn main() {
    const WIDTH: usize = 1080;
    const HEIGHT: usize = 2340;
    const PIXELS: usize = WIDTH * HEIGHT;
    let mut pixels = vec![0u32; 2 * PIXELS];
    let screen = unsafe { display::Display::new(pixels.as_mut_ptr()) };
    screen.clear();
    let labels = LABELS;
    for (index, label) in labels.iter().enumerate() {
        let row = index + 2;
        screen.hex(row, label, 0x01234567);
        let y = 180 + row * 72;
        for buffer in 0..2 {
            let start = buffer * PIXELS + y * WIDTH;
            let end = start + 49 * WIDTH;
            assert!(pixels[start..end].iter().any(|&p| p != 0x00122b52), "row {row}: {label}");
            let digit_x = 48 + (label.len() + 8) * 42;
            assert!((0..49).any(|dy| pixels[start + dy*WIDTH + digit_x..start + dy*WIDTH + digit_x + 35]
                .iter().any(|&p| p != 0x00122b52)), "last digit missing: {label}");
        }
    }
    let before = pixels.clone();
    // Reproduce the late progress callback that obscured the real device panel.
    for phase in [1, 2, 3] { screen.runtime_progress(true, phase, 19, 23, 27); }
    let footer = (180 + 29 * 72) * WIDTH;
    for buffer in 0..2 {
        let start = buffer * PIXELS;
        assert_eq!(&pixels[start..start+footer], &before[start..start+footer]);
    }
    assert_ne!(pixels, before, "footer still reports progress");
    assert_eq!(&pixels[..PIXELS], &pixels[PIXELS..]);
    screen.runtime_progress(false, 1, 19, 23, 27);
    assert_ne!(&pixels[..footer], &before[..footer], "legacy progress remains available");
    println!("PASS: all 27 panel labels and final hex digits rendered in both buffers");
}
'''.replace('LABELS', '&' + json.dumps(labels)))
run(['rustc', '--edition=2024', '-A', 'dead_code', str(harness), '-o', str(out / 'panel-render')], 'panel-render-build')
run([str(out / 'panel-render')], 'panel-render-test')
(out / 'runtime-integration-result.json').write_text(json.dumps(dict(result='passed',
    feature=FEATURE, profiles=profiles, paired_result_sha256=sha(out / 'offline-comparison/result.json'),
    panel_rows=len(labels), native_feature_guard_cases=3, source_sha256={str(path.relative_to(ROOT)): sha(path)
        for path in [source, renderer, Path(__file__).resolve(), ROOT / 'rust/Cargo.toml']},
    device_actions=[], scope='Full feature-profile host tests and source/render checks; actual Pixel binary build and hardware result are separate.'), indent=2) + '\n')
print('PASS: composed independent IPA runtime; no device action.')
