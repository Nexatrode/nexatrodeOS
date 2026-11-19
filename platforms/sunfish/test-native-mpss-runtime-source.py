#!/usr/bin/env python3
"""Manifest-extension regression and actual isolated source-audit CLI."""
from pathlib import Path
import copy
import json
import re
import importlib.util
import subprocess
import sys
import tempfile
import tomllib
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
spec = importlib.util.spec_from_file_location('runtime_source', HERE / 'audit-native-mpss-runtime-source.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class ManifestExtension(unittest.TestCase):

    def test_first_oack_panel_hex_labels_fit_all_rows(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        for title in ('FIRST OACK ELIGIBILITY', 'FIRST OACK GRANT WINDOW', 'FIRST OACK YIELD RX'):
            with self.subTest(panel=title):
                title_call = f'display.replace_line(1, "{title}");'
                self.assertEqual(source.count(title_call), 1)
                panel = source.split(title_call, 1)[1].split('] { display.hex(row, label, value); }', 1)[0]
                fields = re.findall(r'^\s*\((\d+), "([^"]+)",', panel, re.MULTILINE)
                self.assertEqual([int(row) for row, _ in fields], list(range(2, 29)))
                self.assertEqual([label for _, label in fields if len(label.encode()) > 15], [])
    def test_wlan_listener_is_opt_in_and_cannot_restart(self):
        owner = (ROOT / 'rust/src/qcom_modem_owner.rs').read_text()
        probe = owner[owner.index('pub struct WlanStateListener'):]
        self.assertLess(probe.index('self.attempted = true'), probe.index('owner.begin_servreg_notif_listener'))
        self.assertIn('q.result == 0 && q.error == 0 && q.state == Some(ServiceState::Uninit)', probe)
        self.assertIn('self.attempted || !ipa_ready', probe)
        for forbidden in ['begin_servreg_notif_restart_pd(', 'enable_wlfw_client(', 'service_tftp_step(']:
            self.assertNotIn(forbidden, probe)
        screen = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        self.assertIn('previous_wlan_listener !=', screen)
        self.assertNotIn('begin_servreg_notif_restart_pd(', screen)
        self.assertIn('NO PD RESTART OR WLAN', screen)
        for label in ['LISTEN PHASE', 'LISTEN STATE', 'LISTEN RESULT', 'LISTEN ERROR', 'UPDATE TXN', 'UPDATE STATE', 'ACK RESULT', 'ACK ERROR']:
            self.assertLessEqual(len(label), 15)
            self.assertIn('"' + label + '"', screen)
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())['features']
        self.assertEqual(cargo['sunfish-wlan-state-listener'], ['sunfish-wlan-state-query'])
        self.assertNotIn('sunfish-wlan-state-listener', cargo['sunfish-native-startup-mpss-runtime'])

    def test_wlan_query_is_opt_in_one_shot_and_has_no_startup_calls(self):
        screen = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        owner = (ROOT / 'rust/src/qcom_modem_owner.rs').read_text()
        probe = owner[owner.index('pub struct WlanStateQuery'):owner.index('pub struct WlanStateListener')]
        self.assertLess(probe.index('self.attempted = true'), probe.index('owner.begin_servreg_notif_query'))
        self.assertIn('self.attempted || !ipa_ready', probe)
        for forbidden in ['begin_servreg_notif_restart_pd(', 'begin_servreg_notif_listener(', 'enable_wlfw_client(', 'service_tftp_step(']:
            self.assertNotIn(forbidden, probe)
            self.assertNotIn(forbidden, screen)
        self.assertLess(screen.index('owner.enable_servreg_notif_client(0x4005)'), screen.index('owner.start_control()'))
        self.assertIn('self.ipa_report.is_some_and(|r| r.protocol_complete)', screen)
        self.assertIn('previous_wlan_query !=', screen)
        block = screen[screen.index('let q = self.wlan_query.snapshot(owner)'):]
        for label in ['QUERY PHASE', 'PD STATE RAW', 'QUERY RESULT', 'QUERY ERROR', 'NOTIF SERVERS', 'NOTIF NODE', 'NOTIF PORT', 'WLFW SERVERS', 'WLFW NODE', 'WLFW PORT']:
            self.assertLessEqual(len(label), 15)
            self.assertIn('"' + label + '"', block)
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())['features']
        self.assertEqual(cargo['sunfish-wlan-state-query'], ['sunfish-tftp-missing-temp-replies'])
        self.assertNotIn('sunfish-wlan-state-query', cargo['sunfish-native-startup-mpss-runtime'])

    def test_missing_file_overlay_is_gated_and_retains_final_write_result(self):
        screen = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        start = screen.index('let (trace, wire) = match self.stage.as_ref()')
        block = screen[start:screen.index('display.replace_line(29, "RESTART WHEN DONE");', start)]
        self.assertIn('if self.error == 0', screen[start-100:start])
        for label in ['MISSING RRQ', 'MISSING KIND', 'MISS RSP FLAGS', 'MISS PEER PORT', 'MISS LOCAL PORT', 'MISS INTENT']:
            self.assertLessEqual(len(label), 15)
            self.assertIn('"' + label + '"', block)
        self.assertIn('previous_missing !=', screen)
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())['features']
        self.assertEqual(cargo['sunfish-tftp-missing-temp-replies'], ['sunfish-tftp-final-ack-observer'])
        self.assertNotIn('sunfish-tftp-missing-temp-replies', cargo['sunfish-native-startup-mpss-runtime'])

    def test_final_ack_display_is_success_only_and_refreshes_on_trace_changes(self):
        screen = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        start = screen.index('// Keep this after the timing overlays.')
        end = screen.index('display.replace_line(29, "RESTART WHEN DONE");', start)
        summary = screen[start:end]
        self.assertIn('if self.error == 0', summary)
        self.assertGreater(start, screen.index('display.hex(16, "WRITES COMPLETE"'))
        for label in ['FINAL ACK FLAGS', 'ACK PEER NODE', 'ACK PEER PORT', 'ACK LOCAL NODE', 'ACK LOCAL PORT', 'ACK BLOCK', 'ACK INTENT']:
            self.assertLessEqual(len(label), 15)
            self.assertIn('"' + label + '"', summary)
        self.assertIn('owner.tftp_final_ack_trace() != previous_final_ack', screen)
        owner = (ROOT / 'rust/src/qcom_modem_owner.rs').read_text()
        start = owner.index('fn service_tftp_server_check_per_peer(')
        end = owner.index('fn flush_tftp_reply(', start)
        observer = owner[start:end]
        self.assertIn('if reply.completed {', observer)
        self.assertLess(observer.index('let dispatch = server.receive'), observer.index('self.adapter.observe_tftp_final_ack'))
        check_path = observer[observer.index('let dispatch = server.receive'):]
        self.assertLess(check_path.index('self.adapter.observe_tftp_final_ack'), check_path.index('self.finish_tftp_request'))

    def test_ram_transfer_outcomes_survive_timing_overlays_without_hiding_errors(self):
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())
        self.assertEqual(cargo['features']['sunfish-tftp-outcome-summary'], ['sunfish-native-tftp-server-check'])
        self.assertNotIn('sunfish-tftp-outcome-summary', cargo['features']['sunfish-native-startup-mpss-runtime'])
        screen = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        start = screen.index('#[cfg(feature = "sunfish-tftp-outcome-summary")]')
        end = screen.index('display.replace_line(29, "RESTART WHEN DONE");', start)
        summary = screen[start:end]
        self.assertIn('if self.error == 0', summary)
        self.assertGreater(start, screen.rindex('display.hex(16, "DOMAIN PRE MS"'))
        for row, label, expression in [
            (16, 'WRITES COMPLETE', 'self.server_check.completed_count() as u32'),
            (17, 'RAM BYTES', 'self.server_check.total_bytes() as u32'),
            (18, 'FIRST COMPLETE', 'u32::from(self.server_check.complete())'),
            (19, 'ACTIVE SESSIONS', 'self.server_check.active_count() as u32'),
        ]:
            self.assertLessEqual(len(label), 15)
            self.assertIn(f'display.hex({row}, "{label}", {expression});', summary)
        for label in ['QUEUE AGE MS', 'PUBLISH AGE MS', 'RXDONE AGE MS']:
            self.assertIn('"' + label + '"', screen[:start])

    def test_barrier_domain_observer_retains_pre_and_post_validation(self):
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())
        self.assertEqual(cargo['features']['sunfish-tftp-barrier-domain-observer'], ['sunfish-tftp-publish-op-observer', 'sunfish-native-startup-mpss-runtime'])
        self.assertNotIn('sunfish-tftp-barrier-domain-observer', cargo['features']['sunfish-native-startup-mpss-runtime'])
        domain = (ROOT / 'rust/src/sunfish_smmu_native.rs').read_text()
        observed = domain[domain.index('    pub(crate) fn with_glink_observed<'):domain.index('    pub(crate) fn quarantine_glink')]
        parts = ['if self.modem_quarantined', 'self.modem_quarantined = true', 'observe(0)',
                 'self.retained_receive_domain()', 'observe(1)', 'checked?', '.glink(action).map_err(Error::SharedMemory)?',
                 'observe(2)', 'self.retained_receive_domain()', 'observe(3)', 'checked?',
                 'self.modem_quarantined = false']
        position = 0
        for part in parts:
            position = observed.index(part, position) + len(part)
        transport = (ROOT / 'rust/src/sunfish_mpss_linux_transport_native.rs').read_text()
        self.assertIn('if detail == Some(4)', transport)
        self.assertIn('native.with_glink_observed(action', transport)
        screen = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        for label in ('DOMAIN PRE MS', 'DOMAIN POST MS'):
            self.assertIn('"' + label + '"', screen)
            self.assertLessEqual(len(label), 15)

    def test_publish_operation_observer_uses_existing_profiler_clock_samples(self):
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())
        self.assertEqual(cargo['features']['sunfish-tftp-publish-op-observer'], ['sunfish-tftp-publish-io-observer'])
        source = (ROOT / 'rust/src/sunfish_mpss_linux_transport_native.rs').read_text()
        profiled = source[source.index('    fn run_profiled<'):source.index('    fn run<T>')]
        self.assertEqual(profiled.count('clock()'), 2)
        self.assertIn('self.timing.record(category, elapsed)', profiled)
        self.assertIn('self.timing.record_detail(detail, elapsed)', profiled)
        for text in ('self.run_profiled(3, Some(0)', 'self.run_profiled(3, Some(1)',
                     'self.run_profiled(3, Some(2)', 'self.run_profiled(3, Some(3)',
                     'self.run_profiled(3, Some(4)', 'self.run_profiled(3, Some(5)'):
            self.assertEqual(source.count(text), 2 if text == 'self.run_profiled(3, Some(4)' else 1)
        pair = source[source.index('    fn barrier_observe('):source.index('    fn notify_peer(')]
        self.assertEqual(pair.count('self.run_profiled(3, Some(4)'), 1)
        self.assertLess(pair.index('c.glink_barrier()?'), pair.index('c.glink_poll()?'))
        self.assertNotIn('self.barrier(', pair)
        self.assertNotIn('self.observe(', pair)
        self.assertNotIn('clock()', pair)
        screen = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        for label in ('IO LEN MS', 'IO WRITE MS', 'IO HEAD MS', 'IO TAIL MS', 'IO BARR MS', 'IO NOTIFY MS'):
            self.assertIn('"' + label + '"', screen)
            self.assertLessEqual(len(label), 15)

    def test_publish_io_observer_snapshots_existing_counters_on_normal_dispatch(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())
        self.assertEqual(cargo['features']['sunfish-tftp-publish-io-observer'], ['sunfish-tftp-oack-phase-observer'])
        self.assertNotIn('sunfish-tftp-publish-io-observer', cargo['features']['sunfish-native-startup-mpss-runtime'])
        self.assertIn('compile_error!("publication IO observer requires normal router dispatch")', source)
        loop = source[source.index('    fn service_owner('):source.index('    fn notified(&mut self,')]
        self.assertEqual(loop.count('counter()'), 2)
        self.assertLess(loop.index('let before_io ='), loop.index('self.router.dispatch(owner)'))
        self.assertLess(loop.index('self.router.dispatch(owner)'), loop.index('self.oack_publish_io.observe('))
        for label in ('IO OBS MS', 'IO DESC MS', 'IO READ MS', 'IO OTHER MS'):
            self.assertIn('"' + label + '"', source)
            self.assertLessEqual(len(label), 15)

    def test_early_dispatch_is_one_opt_in_turn_after_queued_tftp(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())
        self.assertEqual(cargo['features']['sunfish-tftp-early-dispatch-experiment'], ['sunfish-tftp-early-service-experiment'])
        self.assertNotIn('sunfish-tftp-early-dispatch-experiment', cargo['features']['sunfish-native-startup-mpss-runtime'])
        service = source[source.index('    fn service_pass('):source.index('    fn paint_progress(')]
        self.assertEqual(service.count('self.router.dispatch(owner)'), 1)
        queued = service.index('qcom_modem_owner::TftpServiceProgress::ResponseQueued')
        gate = service.index('#[cfg(feature = "sunfish-tftp-early-dispatch-experiment")]')
        dispatch = service.index('self.router.dispatch(owner)')
        self.assertLess(queued, gate)
        self.assertLess(gate, dispatch)
        self.assertLess(dispatch, service.index('owner.dispatch_service_registry_step'))
        self.assertLess(dispatch, service.index('owner.resolve_pending_tftp_close(&mut self.server_check)'))

    def test_early_tftp_service_is_opt_in_and_once_per_successful_pass(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())
        self.assertEqual(cargo['features']['sunfish-tftp-early-service-experiment'], ['sunfish-native-tftp-server-check'])
        self.assertNotIn('sunfish-tftp-early-service-experiment', cargo['features']['sunfish-native-startup-mpss-runtime'])
        service = source[source.index('    fn service_pass('):source.index('    fn paint_progress(')]
        self.assertEqual(service.count('owner.service_tftp_server_check_step('), 1)
        early = '#[cfg(all(feature = "sunfish-native-tftp-server-check", feature = "sunfish-tftp-early-service-experiment"))]'
        late = '#[cfg(all(feature = "sunfish-native-tftp-server-check", not(feature = "sunfish-tftp-early-service-experiment")))]'
        self.assertEqual(service.count('tftp_service_step!();'), 2)
        self.assertLess(service.index(early), service.index('owner.dispatch_service_registry_step'))
        self.assertLess(service.index('owner.service_linux_ipa_step'), service.index(late))
        self.assertIn(early + '\n        tftp_service_step!();', service)
        self.assertIn(late + '\n        tftp_service_step!();', service)

    def test_oack_phase_observer_uses_existing_counters_and_no_io(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        # The independently gated IPA observer adds clock reads only in its
        # own profile. Keep this check on the original OACK-only path.
        source = re.sub(r'(?m)^ *#\[cfg\(feature = "sunfish-ipa-schedule-trace"\)\]\n *self\.ipa_schedule\.observe\([^\n]+\);\n', '', source)
        # The registration preparation profile alone timestamps the TFTP turn.
        source = re.sub(r'(?m)^ *#\[cfg\(feature = "sunfish-ipa-registration-prep-trace"\)\]\n'
                        r' *(?:let tftp_started = counter\(\);|record_registration_prep_work\([^\n]+\);)\n',
                        '', source)
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())
        self.assertEqual(cargo['features']['sunfish-tftp-oack-phase-observer'], ['sunfish-tftp-timing-observer'])
        self.assertNotIn('sunfish-tftp-oack-phase-observer', cargo['features']['sunfish-native-startup-mpss-runtime'])
        service = source[source.index('    fn service_pass('):source.index('    fn paint_progress(')]
        early_start = service.index('let early_ipa = {')
        early_end = service.index('let early_ipa = false;')
        early = service[early_start:early_end]
        self.assertEqual(early.count('counter()'), 2)
        # The cold profile tries the bounded handoff, then falls back to the
        # same early service; the other cfg compiles only that fallback.
        self.assertIn('#[cfg(feature = "sunfish-ipa-registration-cold-handoff")]\n'
                      '            let result = if cold_ipa_step!() {\n'
                      '                Ok(true)\n'
                      '            } else {\n'
                      '                owner.service_ipa_registration_early(started, &mut self.router)\n'
                      '            };', early)
        self.assertIn('#[cfg(not(feature = "sunfish-ipa-registration-cold-handoff"))]\n'
                      '            let result = owner.service_ipa_registration_early(started, &mut self.router);', early)
        self.assertEqual(early.count('owner.service_ipa_registration_early('), 2)
        self.assertLess(early.rindex('owner.service_ipa_registration_early('),
                        early.index('self.oack_phases.mark(3, ended)'))
        service = service[:early_start] + service[early_end:]
        self.assertIn('if !early_ipa {\n            let started = counter();\n            let ipa_result = owner.service_linux_ipa_step', service)
        cold_check = service.index('let early_ipa = if early_ipa {')
        cold_end = service.index('\n        };', cold_check) + len('\n        };')
        self.assertLess(service.index('tftp_service_step!();'), cold_check)
        self.assertLess(cold_end, service.index('if !early_ipa {'))
        cold_service = service[cold_check:cold_end]
        self.assertIn('cold_ipa_step!()', cold_service)
        self.assertIn('self.router.service_cold_ipa_registration(owner, counter)', service)
        self.assertEqual(service[:cold_check].count('counter()') + service[cold_end:].count('counter()'), 6)
        for index, call in enumerate(('dispatch_service_registry_step', 'dispatch_linux_rmtfs_burst', 'service_linux_ipa_step'), 1):
            self.assertLess(service.index('owner.' + call), service.index(f'self.oack_phases.mark({index}, ended)'))
        loop = source[source.index('    fn service_owner('):source.index('    fn notified(&mut self,')]
        self.assertEqual(loop.count('counter()'), 2)
        self.assertLess(loop.index('self.router.dispatch(owner)'), loop.index('self.oack_phases.begin(owner.tftp_timing().request, ended)'))
        start = source.index('// Host-observed latencies start at owner receipt')
        screen = source[start:source.index('#[cfg(feature = "sunfish-tftp-close-handoff-prototype")]', start)]
        for label in ('DISPATCH MS', 'REGISTRY MS', 'STORAGE MS', 'IPA STEP MS'):
            self.assertIn(label, screen)
            self.assertLessEqual(len(label), 15)
        for forbidden in ('read_volatile', 'write_volatile', '.with_io(', '.notify(', '.schedule('):
            self.assertNotIn(forbidden, screen)

    def test_quiet_oack_is_opt_in_and_preserves_checkpoint_and_error_visibility(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        cargo = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())
        self.assertEqual(cargo['features']['sunfish-tftp-quiet-oack-experiment'], ['sunfish-tftp-timing-observer'])
        self.assertNotIn('sunfish-tftp-quiet-oack-experiment', cargo['features']['sunfish-native-startup-mpss-runtime'])
        paint = source[source.index('    fn paint_progress('):source.index('    fn timed_show(')]
        self.assertLess(paint.index('self.progress_trace.checkpoint(phase)'), paint.index('if self.awaiting_first_oack_completion() { return; }'))
        self.assertLess(paint.index('if self.awaiting_first_oack_completion() { return; }'), paint.index('display.runtime_progress('))
        notify = source[source.index('    fn notified(&mut self,'):source.index('    fn fail(&mut self,')]
        self.assertIn('self.oack_paint_gate.allow(waiting, self.error != 0, counter(), self.hz)', notify)
        self.assertIn('self.fail(state, 0x70);\n                self.show(display);\n                return;', notify)
        self.assertIn('self.fail(state, 0x71);\n            self.show(display);\n            return;', notify)
        self.assertIn('owner.tftp_endpoint_trace() != previous_tftp_endpoint', notify)
        waiting = source[source.index('    fn awaiting_first_oack_completion('):source.index('    fn service_owner(')]
        self.assertIn('let timing = owner.tftp_timing();', waiting)
        self.assertIn('timing.request.is_some() && timing.done.is_none()', waiting)

    def test_tftp_timing_display_uses_retained_matched_observations(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        start = source.index('// Host-observed latencies start at owner receipt')
        screen = source[start:source.index('\n        #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]', start)]
        for label in ('FIRST RRQ KIND', 'WRQ PEER NODE', 'WRQ PEER PORT', 'PEER MATCH', 'QUEUE AGE MS', 'PUBLISH AGE MS', 'RXDONE AGE MS', 'OACK FLAGS'):
            self.assertIn(label, screen)
            self.assertLessEqual(len(label), 15)
        self.assertIn('end.checked_sub(start)', screen)
        self.assertIn('u32::MAX', screen)
        self.assertIn('v.flags & 8 != 0', screen)
        for label in ('PUBLISH CALLS', 'WAIT MASK'):
            self.assertIn(label, screen)
            self.assertLessEqual(len(label), 15)
        notifications = source[source.index('    fn notified(&mut self,'):source.index('    fn show(&self,')]
        self.assertNotIn('tftp_schedule_trace()', notifications, 'scheduler counter changes must not trigger redraws')
        for forbidden in ('read_volatile', 'write_volatile', '.with_io(', '.notify(', '.schedule('):
            self.assertNotIn(forbidden, screen)

    def test_tftp_endpoint_display_is_metadata_only_and_refreshes(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        self.assertIn('owner.tftp_endpoint_trace() != previous_tftp_endpoint', source)
        start = source.index('if self.error == 0 {\n            if let Some(Stage::Services(owner)) = self.stage.as_ref() {\n                if let Some(trace) = owner.tftp_endpoint_trace()')
        screen = source[start:source.index('\n        #[cfg(feature = "sunfish-tftp-close-handoff-prototype")]', start)]
        for label in ('OACK SRC NODE', 'OACK SRC PORT', 'OACK DST NODE', 'OACK DST PORT', 'OACK BYTES', 'OACK FLAGS', 'OACK INTENT'):
            self.assertIn(label, screen)
            self.assertLessEqual(len(label), 15)
        for forbidden in ('read_volatile', 'write_volatile', '.with_io(', '.notify(', '.schedule('):
            self.assertNotIn(forbidden, screen)

    def test_close_handoff_precedes_service_dispatch_and_remains_host_only(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        start = source.index('fn service_owner(')
        end = source.index('\n    fn notified(', start)
        body = source[start:end]
        self.assertLess(body.index('self.router.dispatch(owner)'), body.index('owner.resolve_pending_tftp_close(&mut self.server_check)'))
        self.assertLess(body.index('owner.resolve_pending_tftp_close(&mut self.server_check)'), body.index('self.service_pass(state)?'))
        self.assertIn('compile_error!("close-handoff prototype is not yet validated for native boot; host tests only")', source)

    def test_close_handoff_keeps_owned_reply_until_checked_transport_and_server_commit(self):
        source = (ROOT / 'rust/src/qcom_modem_owner.rs').read_text()
        start = source.index('pub fn resolve_pending_tftp_close(')
        end = source.index('\n    pub fn retired_rmtfs_peer_count', start)
        body = source[start:end]
        self.assertLess(body.index('self.can_cancel_server_check_reply(peer, server)'), body.index('self.adapter.commit_tftp_close(self.generation, peer)'))
        self.assertLess(body.index('self.adapter.commit_tftp_close(self.generation, peer)'), body.index('server.abandon_empty_peer('))
        self.assertLess(body.index('server.abandon_empty_peer('), body.index('self.peer_tftp_replies[slot] = None'))
        self.assertIn('return Err(self.stop(error.into()))', body)
        self.assertIn('return Err(self.stop(Error::Stopped))', body)

    def test_close_commit_counter_is_painted_and_triggers_refresh(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        self.assertIn('owner.tftp_close_commits() != previous_close_commits', source)
        self.assertIn('display.hex(11, "CLOSE COMMITS", owner.tftp_close_commits())', source)
        self.assertLessEqual(len('CLOSE COMMITS'), 15)

    def test_routing_failure_screen_displays_only_retained_header_metadata(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        start = source.index('if let Some(info) = owner.routing_diagnostic()')
        screen = source[start:source.index('\n        if self.reference_pair_count', start)]
        for label in ('WIRE SOURCE', 'WIRE SERVICE', 'WIRE BODY SIZE', 'WIRE MSG ID', 'WIRE TXN', 'WIRE TLV 1', 'WIRE TLV 2',
                      'WIRE DEST', 'WIRE KIND'):
            self.assertIn(label, screen)
            self.assertLessEqual(len(label), 15)
        for forbidden in ('read_volatile', 'write_volatile', '.with_io(', '.notify(', '.schedule('):
            self.assertNotIn(forbidden, screen)

    def test_config_ignore_count_is_visible_without_a_success_receipt(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        self.assertIn('display.hex(28, "CONFIG IGNORED", report.ignored_config_requests)', source)
        self.assertIn('display.hex(27, "IGNORED COUNT", report.ignored_unhandled_requests)', source)
        self.assertIn('display.hex(28, "IGNORED MSG ID", u32::from(id))', source)
        self.assertIn('display.hex(16, "INIT RSP IGN", report.ignored_init_responses)', source)

    def test_session_failure_screen_uses_retained_transport_metadata(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        start = source.index('if let Some(info) = owner.session_failure_diagnostic()')
        screen = source[start:source.index('display.replace_line(29,', start)]
        for label in ('FAIL STAGE', 'FAIL COMMAND', 'FAIL PARAM'):
            self.assertIn(label, screen)
            self.assertLessEqual(len(label), 15)
        for forbidden in ('read_volatile', 'write_volatile', '.with_io(', '.notify(', '.schedule('):
            self.assertNotIn(forbidden, screen)

    def test_uc_wait_screen_only_uses_retained_report(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        marker = source.index('// These are already-retained software counters')
        end = source.index('\n        if let Some(Stage::Services(owner))', marker)
        screen = source[marker:end]
        self.assertIn('!report.pending_record', screen)
        self.assertIn('s.modem_ready && s.indication_requested && !s.qmi_uc_ready', screen)
        self.assertIn('self.receive.ipa_reports()', screen)
        for label in ('UC IRQ NOTICES', 'UC STATUS READS', 'UC INIT OBS', 'UC LAST RESP', 'UC LAST STATUS',
                      'QMI REG RX', 'QMI DRIVER RX', 'INTENT ASK', 'INTENT DENY', 'INTENT READY',
                      'DENY SIZE', 'DENY STATES', 'SLOT STATES', 'SLOT SERVICE', 'SLOT REPLIES'):
            self.assertIn(label, screen)
            self.assertLessEqual(len(label), 15)
        for forbidden in ('read_volatile', 'write_volatile', '.with_io(', '.notify(', '.schedule('):
            self.assertNotIn(forbidden, screen)

    def test_ipa_reference_pair_is_explicit_and_uses_retained_ready_receipt(self):
        native = (ROOT / 'rust/src/pixel_ipa_receive.rs').read_text()
        runtime = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        policy = (ROOT / 'rust/src/pixel_ipa_receive_policy.rs').read_text()
        self.assertIn('let reference_pair = !strict && self.post_modem_ready', native)
        self.assertIn('policy::post_modem_config(self.expected, actual, state, true)', native)
        self.assertIn('cfg!(feature = "sunfish-ipa-postmodem-reference-experiment") && ready.generation() == 1', native)
        self.assertIn('if ready.generation() != 1 { return Err(dispatch::Error::Startup); }', native)
        self.assertEqual(native.count('post_modem_ready: false'), 3)
        self.assertEqual(runtime.count('notify_runtime_badge(state, receive, native, ready, allow_reference_pair, badge)'), 3)
        self.assertEqual(runtime.count('self.dispatch_receive(state, other, captured.is_ok())'), 2)
        self.assertIn('&& allow_reference_pair', native)
        dispatch = runtime[runtime.index('    fn dispatch_receive('):runtime.index('    fn step_opening(')]
        self.assertIn('owner.revalidate().map_err(|_| 0x13u32)?', dispatch)
        self.assertLess(dispatch.index('owner.revalidate()'), dispatch.index('notify_runtime_badge('))
        self.assertIn('!state.rx_running() || !state.route_set()', policy)
        for exact in ('expected.config[2] != 0x30000000', 'expected.config[4] != 0x08',
                      'actual.config[2] != 0', 'actual.config[4] != 0x78'):
            self.assertIn(exact, policy)
        self.assertIn('config(expected, actual, state)', policy)

    def test_receive_failure_is_captured_before_quarantine_and_painted_last(self):
        source = (ROOT / 'rust/src/pixel_mpss_runtime.rs').read_text()
        dispatch = source[source.index('    fn dispatch_receive('):source.index('    fn step_opening(')]
        self.assertIn('pixel_ipa_receive::diagnostic_snapshot(state)', dispatch)
        self.assertIn('ReceiveFailure::capture(badge, 0x11, native_error, &error)', dispatch)
        self.assertIn('ReceiveFailure::capture(badge, 0x12, u32::MAX, &error)', dispatch)
        notified = source[source.index('    fn notified('):source.index('    fn fail(')]
        self.assertLess(notified.index('self.detail = detail'), notified.index('self.fail(state, 0x70)'))
        painter = source[source.index('    fn show('):source.index('\nfn counter()')]
        self.assertGreater(painter.index('if let Some(failure) = self.receive_failure.as_ref()'),
                           painter.index('owner.capacity_diagnostic()'))
        for label in ('RX BADGE LOW', 'RX BADGE HIGH', 'RX SCOPE', 'RX NATIVE ERR', 'RX TRUNC'):
            self.assertIn(label, painter)
        self.assertIn('self.guard_failure = pixel_ipa_receive::guard_diagnostic(state)', dispatch)
        self.assertGreater(painter.index('if let Some((stage, space, offset, observed, expected, mask))'),
                           painter.index('if let Some(failure) = self.receive_failure.as_ref()'))
        for label in ('GUARD STAGE', 'GUARD SPACE', 'GUARD OFFSET', 'GUARD ACTUAL', 'GUARD EXPECT', 'GUARD MASK'):
            self.assertIn(label, painter)
        self.assertIn('self.config_failure = pixel_ipa_receive::config_diagnostic(state)', dispatch)
        self.assertGreater(painter.index('if let Some((actual, mismatch, baseline, last_good, count))'),
                           painter.index('if let Some((stage, space, offset, observed, expected, mask))'))
        for label in ('CONFIG DIFF', 'CLK BASE', 'CLK LAST GOOD', 'GOOD SNAPSHOTS'):
            self.assertIn(label, painter)

    def checker(self):
        f = getattr(audit, 'check_manifest_extension', None)
        self.assertTrue(callable(f), 'source audit needs an explicit exact-manifest-extension check')
        return f

    def pair(self):
        original = tomllib.loads((ROOT / 'rust/Cargo.toml').read_text())
        original['features'].pop('sunfish-native-startup-mpss-runtime', None)
        candidate = copy.deepcopy(original)
        candidate['features']['sunfish-native-startup-mpss-runtime'] = [
            'sunfish-native-startup-mpss-auth', 'sunfish-native-startup-mpss-glink']
        return original, candidate

    def test_only_exact_runtime_feature_extension_is_accepted_without_mutating_inputs(self):
        check = self.checker()
        original, candidate = self.pair()
        before = copy.deepcopy((original, candidate))
        check(original, candidate)
        self.assertEqual((original, candidate), before)

    def test_dependency_feature_target_and_runtime_auth_changes_are_rejected(self):
        check = self.checker()
        original, current = self.pair()
        variants = {}
        variants['dependency version'] = copy.deepcopy(current)
        variants['dependency version']['dependencies']['aes']['version'] = '=0.9.4'
        variants['existing feature'] = copy.deepcopy(current)
        variants['existing feature']['features']['sunfish-native-startup-mpss-auth'].append('extra')
        variants['unrelated feature'] = copy.deepcopy(current)
        variants['unrelated feature']['features']['extra'] = []
        variants['target dependency'] = copy.deepcopy(current)
        variants['target dependency']['target']['cfg(target_os = "none")']['dependencies']['sel4']['rev'] = 'different'
        variants['missing real auth'] = copy.deepcopy(current)
        variants['missing real auth']['features']['sunfish-native-startup-mpss-runtime'] = ['sunfish-native-startup-mpss-glink']
        variants['missing extension'] = copy.deepcopy(original)
        for name, candidate in variants.items():
            with self.subTest(name=name), self.assertRaises(ValueError):
                check(original, candidate)

    def test_concrete_runtime_source_bindings_and_mutations(self):
        texts = audit.load_texts(ROOT)
        audit.check(texts)
        compact = {key: audit.compact(value) for key, value in texts.items()}
        mutations = [
            ('TFTP transport progress does not repaint', 'root', 'owner.tftp_tx_progress()!=previous_tftp_tx', 'false'),
            ('per-peer bytes not retained', 'modem_owner', 'bytes[..reply.len].copy_from_slice(&self.tftp_output[..reply.len]);', '/* response lost */'),
            ('per-peer pending slot unbounded', 'modem_owner', 'peer_tftp_replies:[Option<PeerTftpReply>;MAX_REQUESTS]', 'peer_tftp_replies:[Option<PeerTftpReply>;128]'),
            ('wrong outbound intent command', 'session', 'glink_header(&mutrecord,7,self.config.local_channel,size)', 'glink_header(&mutrecord,9,self.config.local_channel,size)'),
            ('missing intent deadline', 'runtime_diagnostic', 'now.wrapping_sub(started)>=hz*10', 'false'),
            ('intent display uses stale cached value', 'root', 'letintent=matchself.stage.as_ref(){Some(Stage::Services(owner))=>owner.outbound_intent_diagnostic()', 'letintent=matchself.stage.as_ref(){Some(Stage::Services(owner))=>self.intent_status'),
            ('unconditional FIFO intent ack', 'fifo', '#[cfg(feature="sunfish-glink-intent-request-experiment")]8=>8,', '8=>8,'),
            ('no checked gate', 'gate', 'self.value.try_borrow_mut()', 'self.value.borrow_mut()'),
            ('no true auth receipt gate', 'auth', '&&r.ready_irq&&', '&&false&&'),
            ('SERVREG duplicate', 'root', 'owner.prepare_service_registry()', 'owner.prepare_service_registry();owner.prepare_service_registry()'),
            ('RMTFS bind removed', 'root', 'owner.activate_linux_rmtfs(server,&mutself.router)', '/* bind skipped */'),
            ('premature GLINK ACK', 'root', 'letpending=core::mem::take(&mutself.pending);', 'Channel::new(GLINK_CHANNEL).irq_ack();letpending=core::mem::take(&mutself.pending);'),
            ('TFTP runtime service', 'root', 'self.service_owner(state)?;', 'self.prepare_tftp_server();self.service_owner(state)?;'),
            ('observer feature guard removed', 'root', '#[cfg(feature="sunfish-native-tftp-observer")]owner.prepare_tftp_observer(0x5000)', 'owner.prepare_tftp_observer(0x5000)'),
            ('observer port changed', 'root', 'owner.prepare_tftp_observer(0x5000)', 'owner.prepare_tftp_observer(0x4000)'),
            ('wrong GLINK IRQ', 'irq', 'pubconstGLINK_CHANNEL:u8=4;', 'pubconstGLINK_CHANNEL:u8=3;'),
            ('runtime scans invalid channel range', 'root', 'forchannelinchannels.iter(){result|=1u64<<channel.index();}', 'forchannelin0..64{ifchannels.contains(Channel::new(channel)){result|=1<<channel;}}'),
            ('auth scans invalid channel range', 'auth', 'forchannelinchannels.iter(){b|=1u64<<channel.index();}', 'foriin0..64{ifchannels.contains(Channel::new(i)){b|=1u64<<i;}}'),
            ('wrong wake sender', 'irq', 'pubconstCONTINUATION_SEND_CHANNEL:u8=7;', 'pubconstCONTINUATION_SEND_CHANNEL:u8=6;'),
            ('wrong wake receiver', 'irq', 'pubconstCONTINUATION_RECEIVE_CHANNEL:u8=8;', 'pubconstCONTINUATION_RECEIVE_CHANNEL:u8=4;'),
            ('unbounded storage burst', 'rmtfs_owner', 'constSTEPS:usize=2;', 'constSTEPS:usize=8000;'),
            ('bypassed storage step', 'rmtfs_owner', 'self.dispatch_linux_rmtfs_step(s,irqs)?', 'ServiceProgress::Progress'),
            ('wake receiver wrong endpoint', 'system', 'id="8" notify="false"', 'id="9" notify="false"'),
            ('wake has reverse authority', 'system', 'id="8" notify="false"', 'id="8" notify="true"'),
            ('wake signal omitted', 'root', 'Channel::new(irq::CONTINUATION_SEND_CHANNELasusize).notify()', 'drop(irq::CONTINUATION_SEND_CHANNEL)'),
            ('wake sent without pending work', 'root', 'ifself.router.report().continuation_pending{self.progress_trace.sent();ifletSome(display)=&display{self.paint_progress(display,3);}Channel::new(irq::CONTINUATION_SEND_CHANNELasusize).notify();}', 'iftrue{self.progress_trace.sent();ifletSome(display)=&display{self.paint_progress(display,3);}Channel::new(irq::CONTINUATION_SEND_CHANNELasusize).notify();}'),
            ('IPA server inherited', 'ipa_link', '!report.ipa_server_registered', 'report.ipa_server_registered'),
            ('runtime stack too small', 'system', 'stack_size="0x200000"', 'stack_size="0x100000"'),
            ('runtime stack unexpectedly enlarged', 'system', 'stack_size="0x200000"', 'stack_size="0x400000"'),
        ]
        for label, key, before, after in mutations:
            changed = dict(compact)
            if key == 'system':
                changed[key] = texts[key]
            self.assertIn(before, changed[key], label)
            changed[key] = changed[key].replace(before, after, 1)
            with self.subTest(label=label), self.assertRaises(ValueError):
                audit.check(changed)

    def test_actual_source_audit_cli(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / 'audit.json'
            result = subprocess.run([sys.executable, str(HERE / 'audit-native-mpss-runtime-source.py'),
                '--workspace', str(ROOT), '--output', str(output)], cwd=ROOT,
                capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            proof = json.loads(output.read_text())
            self.assertEqual(proof['result'], 'passed')
            self.assertFalse(proof['wlan_firmware_tftp_serving_enabled'])
            self.assertTrue(proof['ipa_server_endpoint_can_be_announced'])
            self.assertEqual(proof['device_actions'], [])

    def test_native_notification_converts_only_delivered_microkit_channels(self):
        for name in ('pixel_mpss_runtime.rs', 'pixel_mpss_auth.rs'):
            source = (ROOT / 'rust/src' / name).read_text()
            self.assertNotIn('for channel in 0..64', source)
            self.assertNotIn('for i in 0..64', source)
            self.assertIn('for channel in channels.iter()', source)

    def test_live_auth_entry_owns_display_before_silent_gates(self):
        auth = audit.compact((ROOT / 'rust/src/pixel_mpss_auth.rs').read_text())
        root = audit.compact((ROOT / 'rust/src/sunfish_native_startup_preflight.rs').read_text())
        start = auth.index('fnstart(')
        runtime_entry = auth[start:auth.index('fnstart_runtime(', start)]
        start_runtime = auth[auth.index('fnstart_runtime(', start):
                             auth.index('fnstart_runtime_after_gate(', start)]
        self.assertIn('ENTRY_ATTEMPTED.store(1,Ordering::Release)', runtime_entry)
        self.assertIn('diagnostics_owned()', root)
        self.assertIn('start_runtime(d,s)', runtime_entry)
        self.assertIn('show_gate_diagnostic(', start_runtime)
        self.assertIn('cfg!(feature="sunfish-native-startup-mpss-runtime-gate-diagnostic")',
                      start_runtime)
        self.assertIn('#[cfg(feature="sunfish-native-startup-mpss-runtime-gate-diagnostic")]return;',
                      start_runtime)
        self.assertIn('#[inline(never)]fnstart_runtime_after_gate', auth)
        self.assertLess(start_runtime.index('show_gate_diagnostic(d,s,cfg!'),
                        start_runtime.index('if!pixel_mpss_preauth::verified()||!healthy(s)'))
        for checkpoint in ('AUTH ENTRY START', 'PREAUTH VERIFIED', 'IPA RX HEALTH',
                           'DOMAIN QUIET', 'RMTFS GENERATION', 'RUNTIME PARTS START'):
            self.assertIn(checkpoint.replace(' ', ''), auth)
        runtime_handler = root[root.index('implHandlerforStartup'):]
        self.assertIn('!pixel_mpss_auth::diagnostics_owned()&&pixel_mpss_preauth::started()',
                      runtime_handler)


if __name__ == '__main__':
    unittest.main()
