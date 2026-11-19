#!/usr/bin/env python3
"""Audit that the pre-SCM RMTFS diagnostic stops before secure invocation."""
from pathlib import Path
import argparse
import json
import re

ROOT = Path(__file__).resolve().parents[2]


def require(ok, message):
    if not ok:
        raise SystemExit(message)


def function_body(source, signature):
    start = source.find(signature)
    require(start >= 0, f'missing function signature: {signature}')
    brace = source.find('{', start)
    require(brace >= 0, f'missing function body: {signature}')
    depth = 0
    for index in range(brace, len(source)):
        if source[index] == '{':
            depth += 1
        elif source[index] == '}':
            depth -= 1
            if depth == 0:
                return source[brace + 1:index]
    raise SystemExit(f'unclosed function: {signature}')


def audit(root):
    cargo = (root / 'rust/Cargo.toml').read_text()
    rmtfs = (root / 'rust/src/sunfish_linux_rmtfs.rs').read_text()
    factory = (root / 'rust/src/pixel_linux_rmtfs.rs').read_text()
    startup = (root / 'rust/src/sunfish_native_startup_preflight.rs').read_text()
    shell = (root / 'platforms/sunfish/build-native-startup-preflight.sh').read_text()

    feature = 'sunfish-native-startup-mpss-runtime-prescm-diagnostic'
    require(re.search(rf'^{feature} = \["sunfish-native-startup-mpss-runtime"\]$', cargo, re.M),
            'diagnostic Cargo feature must depend on the reviewed runtime foundation')

    pool_preflight = function_body(rmtfs, 'pub fn preflight_share_with(')
    resource_preflight = function_body(rmtfs, 'pub fn preflight_share_with(')
    # The first method is the pool implementation; the second occurrence is the
    # Resources wrapper. Parse both explicitly from their impl blocks.
    impl_pool = rmtfs.index('impl<I: Io> Pool<I>')
    impl_resources = rmtfs.index('impl<\'a, I: Io> Resources<\'a, I>')
    pool_preflight = function_body(rmtfs[impl_pool:], 'pub fn preflight_share_with(')
    resource_preflight = function_body(rmtfs[impl_resources:], 'pub fn preflight_share_with(')
    require(re.search(r'self\.io\s*\.stage\(&call\)', pool_preflight)
            and 'self.io.invoke' not in pool_preflight,
            'pool diagnostic must stage but never invoke the SCM adapter')
    require('self.pool.preflight_share_with' in resource_preflight
            and '.share()' not in resource_preflight
            and resource_preflight.index('StorageFinalValidationDone')
            < resource_preflight.index('ShareCheckpoint::ReadyBeforeScm'),
            'Resources diagnostic must use preflight and must not call ordinary share')

    factory_preflight = function_body(factory, 'pub(super) fn preflight_share_with(')
    require('resources.preflight_share_with(checkpoint)' in factory_preflight
            and 'self.fault(0x0a)' in factory_preflight,
            'Factory must fail closed on a preflight error')
    for label in (
        'STORAGE CHECK', 'POOL CHECK', 'CALL BUILD', 'SCRATCH STAGE',
        'POST STAGE CHECK', 'READY BEFORE SCM',
    ):
        require(label in startup, f'missing display checkpoint label: {label}')
    require('d.flush();' in startup[startup.index('fn share_checkpoint_label'):],
            'checkpoint renderer must flush before returning to a potentially blocking operation')
    require('preflight_share_with(|checkpoint|' in startup,
            'startup diagnostic does not invoke the preflight-only API')
    require('NO RMTFS ASSIGN' in startup,
            'screen does not state that the RMTFS assignment is withheld')
    require('PRESCM_DIAGNOSTIC_OWNS_DISPLAY.store(true, Ordering::Release)' in startup
            and 'PRESCM_DIAGNOSTIC_OWNS_DISPLAY.load(Ordering::Acquire)' in startup,
            'diagnostic checkpoints are not protected from IRQ display repaint')
    require('static RETAIN_AUTH_PROFILE_LAYOUT: fn(&Display, &mut Startup) = pixel_mpss_auth::start;'
            in startup,
            'shared runtime profile symbols are not retained without calling auth')
    require('not(feature = "sunfish-native-startup-mpss-runtime-prescm-diagnostic")' in startup,
            'diagnostic feature must compile out normal MPSS continuation')
    require('if state._linux_rmtfs.as_ref().unwrap().phase() == Phase::Sharing' in startup,
            'diagnostic must stop the ordinary loop before Pool::share')

    require('mpss-runtime-prescm-diagnostic) feature="sunfish-native-startup-mpss-runtime-prescm-diagnostic"'
            in shell,
            'startup builder has no dedicated pre-SCM mode')
    require('SUNFISH_SKIP_NATIVE_TESTS' in shell,
            'dedicated diagnostic build cannot skip broad test suites')
    require('if [ "$skip_tests" = false ] && [ "$runtime_mode" = true ]; then' in shell,
            'runtime tests must be gated for the pre-SCM diagnostic build')

    return {
        'result': 'passed',
        'device_actions': [],
        'feature': feature,
        'checkpoints': [
            'storage validation start/done', 'pool validation start/done',
            'call construction start/done', 'scratch stage start/done',
            'post-stage validation start/done', 'ready before SCM',
        ],
        'scm_invocation_in_diagnostic': False,
        'pas4_reset_glink_qrtr_wlan_flash_retry': False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, default=ROOT)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    result = audit(args.workspace.resolve())
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + '\n')
    print('PASS: pre-SCM checkpoints are present; diagnostic contains no SCM invocation or device action')


if __name__ == '__main__':
    main()
