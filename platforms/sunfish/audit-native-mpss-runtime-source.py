#!/usr/bin/env python3
"""Audit the checked post-auth GLINK/QRTR runtime and its bounded authority."""
from pathlib import Path
import argparse
import copy
import hashlib
import json
import re
import tomllib
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
PATHS = {
    'root': 'rust/src/pixel_mpss_runtime.rs',
    'entry': 'rust/src/sunfish_native_startup_preflight.rs',
    'auth': 'rust/src/pixel_mpss_auth.rs',
    'session': 'rust/src/qcom_glink_qmi.rs',
    'fifo': 'rust/src/qcom_glink_fifo.rs',
    'runtime_diagnostic': 'rust/src/pixel_mpss_runtime_diagnostic.rs',
    'ipa_link': 'rust/src/sunfish_ipa_linux_qmi_link.rs',
    'ipa_owner': 'rust/src/sunfish_ipa_linux_qmi_owner.rs',
    'qrtr_flow': 'rust/src/qcom_qrtr_flow.rs',
    'qrtr_flow_test': 'tests/sunfish_qrtr_flow_test.rs',
    'qrtr_flow_oracle': 'platforms/sunfish/test-native-qrtr-flow.py',
    'gate': 'rust/src/sunfish_mpss_live_domain.rs',
    'transport': 'rust/src/sunfish_mpss_linux_transport_native.rs',
    'rmtfs_owner': 'rust/src/sunfish_linux_rmtfs_owner.rs',
    'tftp_check': 'rust/src/qcom_tftp_server_check.rs',
    'modem_owner': 'rust/src/qcom_modem_owner.rs',
    'irq': 'rust/src/pixel_mpss_runtime_irq.rs',
    'system': 'platforms/sunfish/native-startup-mpss-runtime.system',
    'auth_system': 'platforms/sunfish/native-startup-mpss-auth.system',
    'validator': 'platforms/sunfish/validate-native-mpss-runtime.py',
    'build': 'platforms/sunfish/build-native-startup-preflight.sh',
    'cargo': 'rust/Cargo.toml',
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(value, message):
    if not value:
        raise ValueError(message)


def compact(text):
    return ''.join(re.sub(r'//[^\n]*', '', text).split())


def check_manifest_extension(reference, candidate):
    current = copy.deepcopy(candidate)
    extension = current.get('features', {}).pop('sunfish-native-startup-mpss-runtime', None)
    require(extension == ['sunfish-native-startup-mpss-auth', 'sunfish-native-startup-mpss-glink'],
            'exact auth plus native GLINK runtime feature')
    require(current == reference, 'runtime extension changed an existing manifest setting or dependency')


def tree(value):
    return (value.tag, tuple(sorted(value.attrib.items())), (value.text or '').strip(),
            tuple(tree(child) for child in value))


def check(texts):
    t = {key: compact(value) for key, value in texts.items()}
    root, entry, auth = t['root'], t['entry'], t['auth']
    session, ipa_link, gate, transport, irq = (t[k] for k in ('session', 'ipa_link', 'gate', 'transport', 'irq'))

    cargo = tomllib.loads(texts['cargo'])
    require(cargo['features'].get('sunfish-native-startup-mpss-runtime') == [
        'sunfish-native-startup-mpss-auth', 'sunfish-native-startup-mpss-glink'],
        'runtime feature is the exact auth plus GLINK extension')
    require(cargo['features'].get('sunfish-qrtr-rmtfs-flow-experiment') == [],
            'QRTR flow experiment is an explicit opt-in')
    require(cargo['features'].get('sunfish-glink-intent-request-experiment') == [] and
            'sunfish-glink-intent-request-experiment' not in cargo['features']['sunfish-native-startup-mpss-runtime'],
            'outbound GLINK intent experiment is opt-in')
    for token in ['OutboundIntentRequest::Idle', 'OutboundIntentRequest::Waiting',
                  'OutboundIntentRequest::Done', 'glink_header(&mutrecord,7,self.config.local_channel,size)',
                  'Err(WriteError::Uncertain)=>returnErr(Error::PublicationUncertain)',
                  'RemoteIntentRequestAck{granted:parameter==1}']:
        require(token in session, 'bounded outbound GLINK intent request: ' + token)
    publishing = session[session.index('pubfnpublish_next<F>('):session.index('fnnext_transmit(&self)')]
    require(publishing.index('glink_header(&mutrecord,7,self.config.local_channel,size)') <
            publishing.index('letdone_before_ack='), 'one intent request precedes ordinary receive completion')
    for token in ['self.next_transmit().is_none()', 'self.intent_request_ack.is_none()',
                  '!self.receive.iter().any(|rx|rx.state==RxState::Advertising)',
                  'self.outbound_intent_attempts.saturating_add(1)', 'self.outbound_intent_blocked.saturating_add(1)']:
        require(token in publishing, 'outbound intent eligibility and diagnostics: ' + token)
    require('#[cfg(feature="sunfish-glink-intent-request-experiment")]8=>8,' in t['fifo'],
            'FIFO command 8 is experiment-gated')
    for token in ['intent_deadline.update(self.intent_status.0,counter(),self.hz)',
                  'self.fail(state,0x75)', 'Channel::new(irq::CONTINUATION_SEND_CHANNELasusize).notify()',
                  'diagnostic::intent_deadline_only(',
                  'Some(Stage::Services(owner))=>owner.outbound_intent_diagnostic()']:
        require(token in root, 'bounded deadline and wake: ' + token)
    require('now.wrapping_sub(started)>=hz*10' in t['runtime_diagnostic'], 'ten-second request deadline')
    show = root[root.index('fnshow(&self,display:&Display)'):]
    require('Some(Stage::Services(owner))=>owner.outbound_intent_diagnostic()' in show,
            'intent display reads retained state even after dispatch failure')
    require(cargo['features'].get('sunfish-qrtr-ipa-flow-experiment') == [
        'sunfish-qrtr-rmtfs-flow-experiment', 'sunfish-linux-ipa-qmi'],
        'IPA QRTR flow uses the explicit credit model and IPA client')
    require('sunfish-qrtr-ipa-flow-experiment' not in cargo['features']['sunfish-native-startup-mpss-runtime'],
            'IPA QRTR flow is not enabled by the normal runtime feature')
    for token in ['SUNFISH_NATIVE_IPA_QRTR_FLOW_EXPERIMENT', 'test-native-ipa-qrtr-flow.py']:
        require(token in texts['build'], 'explicit IPA QRTR experiment build and tests: ' + token)
    require(cargo['features'].get('sunfish-qrtr-deferred-requests') == ['sunfish-qrtr-rmtfs-flow-experiment'],
            'deferred requests require the explicit QRTR flow experiment')
    require('sunfish-qrtr-deferred-requests' not in cargo['features']['sunfish-native-startup-mpss-runtime'],
            'deferred requests are not enabled by the normal runtime feature')
    require(cargo['features'].get('sunfish-qrtr-servreg-flow-experiment') == ['sunfish-qrtr-rmtfs-flow-experiment'],
            'SERVREG reference flow remains separately opt-in')
    require('sunfish-qrtr-servreg-flow-experiment' not in cargo['features']['sunfish-native-startup-mpss-runtime'],
            'normal runtime does not enable SERVREG reference flow')
    for token in ['SUNFISH_NATIVE_SERVREG_QRTR_FLOW_EXPERIMENT', 'test-native-servreg-qrtr-flow.py']:
        require(token in texts['build'], 'explicit SERVREG reference flow build and tests: ' + token)
    require(cargo['features'].get('sunfish-ipa-ignore-config-reference') == ['sunfish-linux-ipa-qmi'],
            'IPA CONFIG reference behavior remains separately opt-in')
    require('sunfish-ipa-ignore-config-reference' not in cargo['features']['sunfish-native-startup-mpss-runtime'],
            'normal runtime does not ignore IPA CONFIG by default')
    for token in ['SUNFISH_NATIVE_IPA_IGNORE_CONFIG_REFERENCE', 'test-native-ipa-ignore-config.py']:
        require(token in texts['build'], 'explicit IPA CONFIG reference build and tests: ' + token)
    require(cargo['features'].get('sunfish-ipa-ignore-unhandled-reference') == ['sunfish-ipa-ignore-config-reference'],
            'unhandled IPA reference behavior remains separately opt-in')
    require('sunfish-ipa-ignore-unhandled-reference' not in cargo['features']['sunfish-native-startup-mpss-runtime'],
            'normal runtime does not ignore unhandled IPA requests by default')
    require('SUNFISH_NATIVE_IPA_IGNORE_UNHANDLED_REFERENCE' in texts['build'] and '--unhandled' in texts['build'],
            'unhandled IPA reference build and tests are explicit')
    require(cargo['features'].get('sunfish-ipa-ignore-init-response-reference') == ['sunfish-ipa-ignore-unhandled-reference'],
            'IPA init response reference behavior remains separately opt-in')
    require('sunfish-ipa-ignore-init-response-reference' not in cargo['features']['sunfish-native-startup-mpss-runtime'],
            'normal runtime does not ignore IPA init responses by default')
    require('SUNFISH_NATIVE_IPA_IGNORE_INIT_RESPONSE_REFERENCE' in texts['build'] and '--init-response' in texts['build'],
            'IPA init response reference build and tests are explicit')
    require(cargo['features'].get('sunfish-ipa-postmodem-reference-experiment') == [],
            'post-modem IPA reference experiment is an explicit opt-in')
    require('SUNFISH_NATIVE_IPA_POSTMODEM_REFERENCE' in texts['build'],
            'post-modem IPA reference experiment has an explicit build opt-in')
    for token in ['SUNFISH_NATIVE_QRTR_DEFERRED_REQUESTS', 'test-native-qrtr-deferred.py']:
        require(token in texts['build'], 'explicit deferred request build and tests: ' + token)
    for token in ['pubconstLOW_WATERMARK:u8=5;', 'pubconstHIGH_WATERMARK:u8=10;',
                  'pubfnfinish_after_glink(', 'pubfnfail_uncertain(', 'pubfnresume(']:
        require(token in t['qrtr_flow'], 'bounded QRTR flow model: ' + token)
    for token in ['SUNFISH_NATIVE_QRTR_FLOW_EXPERIMENT', 'test-native-qrtr-flow.py']:
        require(token in texts['build'], 'explicit QRTR experiment build and tests: ' + token)

    profile_start = session.index('pubfnnative_runtime_profile(')
    profile_end = session.index('#[cfg(feature="sunfish-native-startup-mpss-runtime")]pubfnruntime_profile_report', profile_start)
    profile = session[profile_start:profile_end]
    for token in ['register(generation,Service::RemoteStorage,0x4000)?',
                  'enable_linux_ipa(generation,0x4004)?', 'session.runtime_profile=true;']:
        require(token in profile, 'runtime QRTR session profile: ' + token)
    for forbidden in ['ServiceRegistry', 'Service::IpaServer', 'prepare_tftp', 'tftp_port']:
        require(forbidden not in profile, 'inherited early publication in runtime session: ' + forbidden)

    prepared = ipa_link[ipa_link.index('pubfnlinux_ipa_prepared('):ipa_link.index('pub(super)fnlinux_ipa_port_in_use', ipa_link.index('pubfnlinux_ipa_prepared('))]
    for token in ['ifself.runtime_profile{', '!report.remote_storage_ready',
                  '!report.service_registry_registered', '!report.ipa_server_registered',
                  'report.tftp_port.is_none()', '!report.tftp_server_prepared']:
        require(token in prepared, 'runtime attach checks bounded profile: ' + token)
    require('self.control.status().ready_services&(1<<4)!=0' in prepared and
            'self.router.endpoint(Service::IpaServer).is_some()' in prepared,
            'auth-only/legacy IPA attach requirements remain intact')

    for token in ['NativeGate<T>', 'self.value.try_borrow_mut()', 'self.quarantined.set(true)',
                  'drop(value);', '#![forbid(unsafe_code)]']:
        require(token in gate, 'checked one-owner RefCell gate: ' + token)
    for token in ['gate:NativeGate<Native<', 'gate.try_borrow_mut(action)',
                  "pubfnstart_live<'m,F:FnMut", 'fifo::BootTransport<Retained']:
        require(token in transport, 'transport retains the sole checked gate: ' + token)

    for token in ['ManuallyDrop<pixel_mpss_auth::NativeBoot>', 'ReadyObservation',
                  'ManuallyDrop<pixel_ipa_receive::Runtime>', 'Stage::Opening(opening)',
                  'Stage::Services(owner)', 'opening.with_io(|io|', 'io.with_native(|native|',
                  'pixel_ipa_receive::notify_runtime_badge',
                  'pixel_ipa_receive::validate_runtime_setup',
                  'owner.prepare_service_registry()', 'owner.prepare_ipa_server_endpoint(IPA_SERVER_PORT)',
                  'owner.start_control()', 'owner.enable_ipa_server(&permit,&mutself.router)',
                  'owner.activate_linux_rmtfs(server,&mutself.router)', 'self.service_owner(state)?',
                  'owner.dispatch_service_registry_step(&mutself.router)',
                  'owner.dispatch_linux_rmtfs_burst(server,&mutself.router)',
                  'owner.service_linux_ipa_step(started,&mutself.router)',
                  'self.router.capture(badge)', 'self.router.take_other()',
                  'self.dispatch_receive(state,other,captured.is_ok())', 'self.router.quarantine()',
                  'pixel_ipa_receive::quarantine_runtime', 'NOTFTPORWLFWSTART',
                  'NOWLANORNETWORKFRAMES']:
        require(token in root, 'concrete retained runtime path: ' + token)
    burst = t['rmtfs_owner'][t['rmtfs_owner'].index('pubfndispatch_linux_rmtfs_burst'):
                             t['rmtfs_owner'].index('fnbind_linux_rmtfs')]
    for token in ['constSTEPS:usize=2;', 'for_in0..STEPS{',
                  'self.dispatch_linux_rmtfs_step(s,irqs)?',
                  'ServiceProgress::Progress=>{}', 'other=>returnOk(other)']:
        require(token in burst, 'bounded storage burst retains individual checks: ' + token)
    require(root.count('prepare_service_registry()') == 1,
            'SERVREG is staged at one runtime call site')
    activation = root[root.index('fnactivate_services('):root.index('fnservice_pass(')]
    promotion = root[root.index('fnpromote('):root.index('fnactivate_services(')]
    require(activation.index('pixel_ipa_receive::validate_runtime_setup') <
            activation.index('owner.prepare_ipa_server_endpoint(IPA_SERVER_PORT)') <
            activation.index('owner.start_control()') <
            activation.index('owner.enable_ipa_server(&permit,&mutself.router)') <
            activation.index('owner.activate_linux_rmtfs(server,&mutself.router)'),
            'IPA hardware guard, endpoint staging and announcement order')
    require(activation.index('owner.prepare_service_registry()') < activation.index('owner.start_control()') <
            activation.index('owner.activate_linux_rmtfs(server,&mutself.router)'),
            'SERVREG staging and RMTFS binding order')
    require(promotion.index('self.activate_services(state)?') < promotion.index('self.service_owner(state)?'),
            'real RMTFS bind completes before first QRTR service pass')
    require('Channel::new(GLINK_CHANNEL).irq_ack()' not in root,
            'no GLINK ACK before a delivered router notification')
    for name, source in [('runtime', root), ('auth', auth)]:
        require('forchannelinchannels.iter(){' in source and
                'Channel::new(channel)' not in source and 'fori in0..64{' not in source,
                name + ' notification iterates only delivered Microkit channels')
    observer = '#[cfg(feature="sunfish-native-tftp-observer")]owner.prepare_tftp_observer(0x5000).map_err(|_|0x49u32)?;'
    require(root.count(observer) == 1 and activation.index(observer) < activation.index('owner.start_control()'),
            'opt-in metadata-only TFTP observer is staged once before control')
    require(cargo['features'].get('sunfish-native-tftp-observer') == ['sunfish-native-startup-mpss-runtime'],
            'observer requires explicit runtime feature')
    require('sunfish-native-tftp-observer' not in cargo['features']['sunfish-native-startup-mpss-runtime'],
            'observer is not part of the default runtime')
    server_check = '#[cfg(feature="sunfish-native-tftp-server-check")]owner.prepare_tftp_server(0x5000).map_err(|_|0x4au32)?;'
    require(root.count(server_check) == 1 and root.count('prepare_tftp_server(') == 1 and
            activation.index(server_check) < activation.index('owner.start_control()'),
            'opt-in server-check endpoint is staged once before control')
    require(cargo['features'].get('sunfish-native-tftp-server-check') == ['sunfish-native-startup-mpss-runtime'] and
            'sunfish-native-tftp-server-check' not in cargo['features']['sunfish-native-startup-mpss-runtime'],
            'server-check is an explicit runtime opt-in')
    require(root.count('owner.service_tftp_server_check_step(') == 1 and 'owner.service_tftp_step(' not in root,
            'runtime dispatches only the restricted server-check handler')
    require('owner.tftp_tx_progress()!=previous_tftp_tx' in root and
            all(label in root for label in ['"TFTPTXSTART"', '"TFTPTXSENT"', '"TFTPTXDONE"']),
            'TFTP transport progress refreshes its distinct counters')
    require('compile_error!("observerandserver-checkmodesaremutuallyexclusive")' in root,
            'TFTP profiles are mutually exclusive')
    tftp_check = t['tftp_check']
    require('#[cfg(not(feature="sunfish-tftp-extended-sessions-experiment"))]pubconstTRANSFER_COUNT:usize=4;' in tftp_check and
            '#[cfg(feature="sunfish-tftp-extended-sessions-experiment")]pubconstTRANSFER_COUNT:usize=32;' in tftp_check and
            cargo['features'].get('sunfish-tftp-extended-sessions-experiment') == ['sunfish-native-tftp-server-check'] and
            'sunfish-tftp-extended-sessions-experiment' not in cargo['features']['sunfish-native-tftp-server-check'],
            'extended TFTP sessions remain bounded and explicitly opted in')
    for token in ['pubconstSERVICE_PORT:u32=0x5000;', 'pubconstTRANSFER_PORT:u32=0x5001;', 'pubconstTRANSFER_COUNT:usize=4;',
                  'pubconstMAX_DATA:usize=512;', 'pubconstRAM_CAPACITY:usize=8192;',
                  'b"\\0\\x02/readwrite/server_check.txt\\0"',
                  'route.source.node!=0', 'route.destination.node!=1',
                  '!is_transfer_port(route.destination.port)', 'transfer.peer!=Some(route.source)', 'data.len()>MAX_DATA',
                  'transfer.bytes[transfer.len..end].copy_from_slice(data)',
                  'transfer.complete=data.len()<MAX_DATA;', '#![forbid(unsafe_code)]']:
        require(token in tftp_check, 'bounded RAM-only TFTP write model: ' + token)
    tftp_owner = t['modem_owner']
    owner_start = tftp_owner.index('pubfnservice_tftp_server_check_step(')
    owner_end = tftp_owner.index('fnflush_tftp_reply(', owner_start)
    check_owner = tftp_owner[owner_start:owner_end]
    require(check_owner.index('self.pending_tftp_reply=Some(') < check_owner.index('self.finish_tftp_request(') <
            check_owner.rindex('self.flush_tftp_reply('), 'retain TFTP response before releasing input')
    for token in ['#[cfg(feature="sunfish-native-tftp-server-check")]ifmatches!(rx,Receive::Session(Event::TftpRequest(_)))||(self.has_pending_tftp_reply()&&matches!(rx,Receive::Session(Event::TransmitDone))){break;}',
                  'self.queue_full_kind=1;', 'self.queue_full_kind=2;']:
        require(token in tftp_owner, 'TFTP yield and queue diagnostic: ' + token)
    require(cargo['features'].get('sunfish-tftp-per-peer-reply-experiment') == ['sunfish-native-tftp-server-check'] and
            'sunfish-tftp-per-peer-reply-experiment' not in cargo['features']['sunfish-native-tftp-server-check'],
            'per-peer retained replies are an explicit opt-in')
    require(cargo['features'].get('sunfish-tftp-first-rrq-observer') == ['sunfish-native-tftp-server-check'] and
            'sunfish-tftp-first-rrq-observer' not in cargo['features']['sunfish-native-tftp-server-check'] and
            'ifopcode==tftp::OP_RRQ&&self.first_rrq.is_none(){self.first_rrq=tftp::observe_request(packet).ok();}' in tftp_owner,
            'first parsed RRQ capture is bounded metadata and an explicit opt-in')
    require(cargo['features'].get('sunfish-tftp-deferred-close-experiment') ==
            ['sunfish-native-tftp-server-check', 'sunfish-qrtr-deferred-requests'] and
            'sunfish-tftp-deferred-close-experiment' not in cargo['features']['sunfish-native-tftp-server-check'],
            'deferred TFTP close is an explicit opt-in')
    close = t['session'][t['session'].index('fnreceive_control('):t['session'].index('fnobserve_client_control(')]
    for token in ['matching==1', 'owner_clear&&!other_receive&&!other_transmit',
                  '&rx.bytes[32..32+request.len()]==request',
                  '!self.rmtfs_flow_destinations.contains(&Some(peer))',
                  'self.rmtfs_reference_flows.close(key).is_err()',
                  'self.retired_rmtfs_peers[tombstone.unwrap()]=Some(peer);',
                  'self.receive[first].state=RxState::Control;', 'self.receive[first].done_pending=true;']:
        require(token in close, 'bounded deferred-close check: ' + token)
    work = tftp_owner[tftp_owner.index('fnwork('):]
    require(work.index('self.adapter.set_tftp_owner_busy(self.generation,busy)?;') <
            work.index('letrx=self.adapter.poll_receive(self.generation)?;') and
            work.index('self.adapter.set_tftp_owner_peers(self.generation,peers)?;') <
            work.index('letrx=self.adapter.poll_receive(self.generation)?;'),
            'owner pending-work snapshots are refreshed before polling')
    require(cargo['features'].get('sunfish-tftp-peer-close-experiment') == ['sunfish-tftp-deferred-close-experiment'] and
            'self.tftp_owner_snapshot_valid&&!self.tftp_owner_peers.contains(&Some(peer))' in close and
            'tftp_owner_snapshot_valid:false' in t['session'] and
            'TFTP_OWNER_PEER_SLOTS:usize=MAX_REQUESTS*2+1;' in t['session'],
            'peer-specific close remains opt-in, bounded and fail-closed before the first snapshot')
    for token in ['peer_tftp_replies:[Option<PeerTftpReply>;MAX_REQUESTS]',
                  'self.peer_tftp_replies[free]=Some(PeerTftpReply{',
                  'bytes[..reply.len].copy_from_slice(&self.tftp_output[..reply.len]);',
                  'reply.meta.route.destination==route.destination',
                  'self.adapter.queue_tftp_response(self.generation,reply.meta.route,&reply.bytes[..reply.meta.bytes])',
                  'self.peer_tftp_replies[slot]=None;',
                  'self.peer_tftp_cursor=(slot+1)%MAX_REQUESTS;']:
        require(token in tftp_owner, 'bounded per-peer reply handling: ' + token)
    per_peer = tftp_owner[tftp_owner.index('fnservice_tftp_server_check_per_peer('):tftp_owner.index('fnflush_tftp_reply(')]
    require(per_peer.index('self.peer_tftp_replies[free]=Some(PeerTftpReply{') < per_peer.index('self.finish_tftp_request(&handle,interrupts)?'),
            'per-peer response retained before releasing input')
    for token in ['ifletSome(reply)=self.pending_tftp_reply',
                  'route.source.port==crate::qcom_tftp_server_check::SERVICE_PORT',
                  'route.destination==reply.route.destination',
                  'crate::qcom_tftp_server_check::is_transfer_port(reply.route.source.port)',
                  'self.tftp_output[..reply.bytes]==*crate::qcom_tftp_server_check::OACK',
                  'packet==crate::qcom_tftp_server_check::REQUEST',
                  'held_route==route&&held_packet==crate::qcom_tftp_server_check::REQUEST',
                  'self.adapter.finish_tftp_request(self.generation,&handle)?']:
        require(token in tftp_owner, 'bounded duplicate WRQ handling: ' + token)
    snapshot = session[session.index('pubfntftp_queue_snapshot(&self)'):session.index('pubfncapacity_diagnostic', session.index('pubfntftp_queue_snapshot(&self)'))]
    for token in ['RxState::Tftp', 'TxKind::TftpReply', 'tx.glink_done', 'tx.qrtr_done']:
        require(token in snapshot, 'software-only TFTP queue snapshot: ' + token)
    for forbidden in ['.gate(', '.revalidate(', 'read_volatile', 'write_volatile', 'queue_tftp_response', 'finish_tftp_request']:
        require(forbidden not in snapshot, 'TFTP snapshot must not touch transport or hardware: ' + forbidden)
    require('#[cfg(feature="sunfish-native-tftp-server-check")]queue_full_kind:0,' in t['ipa_owner'],
            'IPA-enabled owner initializes queue diagnostic')
    for forbidden in ['begin_wlfw',
                      'activate_tftp', 'firmware_bytes', 'seL4_ARM_SMC_Call', 'copy_nonoverlapping']:
        require(forbidden not in root, 'runtime exceeds bounded QRTR discovery: ' + forbidden)

    for token in ['ifpixel_mpss_auth::runtime_ready(&state){pixel_mpss_runtime::start(&d,&mutstate);}',
                  'ifself._mpss_runtime.is_some(){pixel_mpss_runtime::notified(self,channels);']:
        require(token in entry, 'startup transfers notifications only after auth readiness: ' + token)
    require(entry.index('pixel_mpss_auth::start(&d,&mutstate)') <
            entry.index('pixel_mpss_runtime::start(&d,&mutstate)'), 'auth starts before post-auth runtime')
    auth_handoff = auth[auth.index('fntake_runtime('):auth.index('fnviews()')]
    require('#[inline(never)]fnstart_runtime_after_gate' in auth,
            'keep PAS4/runtime tail out of the pre-gate stack frame')
    for token in ['fnshow_loader_checkpoint(d:&Display,r:&Runtime,label:&str,detail:Option<(&str,u32)>,)',
                  '"SERVICESTART"', '"SERVICERETURNED"', '"POWERRESOURCE"',
                  '"RESOURCESREADY"', '"LOADERSTEPENTER"', '"LOADERSTEPRETURNED"',
                  'fnretained_domain_quiet(s:&Startup,d:Option<&Display>)->bool',
                  'domain.retained_quiet_with(|stage,index|', '"DOMFSRGLOBAL"',
                  '"DOMCBCONTROL"', '"DOMFSRCONTEXT"', '"RXRECHECK"',
                  'fnhealthy_with_progress(s:&Startup,d:Option<&Display>)->bool',
                  '"RXPENDINGSTART"', '"RXHEALTHCHECK"']:
        require(token in auth, 'early PAS loader boundary checkpoint: ' + token)
    for token in ['r.phase==Phase::Ready', 'r.ready_irq', 'r.cold_peer_verified',
                  'r.auth.as_ref().is_some_and(|a|a.generation()==GENERATION)',
                  'r.ready.as_ref().is_some_and(|v|v.generation()==GENERATION)',
                  'phase=Phase::Delegated;', 'ReceiptHandoff::new(',
                  'PENDING.swap(0,Ordering::AcqRel)|DEFERRED.swap(0,Ordering::AcqRel)']:
        require(token in auth_handoff, 'one-use actual auth/ready handoff: ' + token)

    for token in ['pubconstGLINK_CHANNEL:u8=4;', 'pubconstSMP_CHANNEL:u8=5;',
                  'pubconstWATCHDOG_CHANNEL:u8=6;', 'self.other|=physical_badge&OTHER;',
                  'self.pending|=incoming;',
                  'Interrupts::acknowledge(self,modem::GLINK_CHANNEL).map_err(|_|Error::Ack)']:
        require(token in irq, 'preserved and checked IRQ routing: ' + token)
    require('irqirq="481"id="4"trigger="edge"' in t['system'], 'one GLINK IRQ481/channel4 edge')
    require('irqirq="483"id="5"trigger="edge"' in t['system'] and
            'irqirq="298"id="6"trigger="edge"' in t['system'], 'preserve SMP2P and watchdog owners')
    runtime_xml = ET.fromstring(texts['system'])
    auth_xml = ET.fromstring(texts['auth_system'])
    irq_node = runtime_xml.find("protection_domain/irq[@irq='481']")
    require(irq_node is not None, 'GLINK IRQ present exactly once')
    runtime_pd = runtime_xml.find('protection_domain')
    auth_pd = auth_xml.find('protection_domain')
    require(int(runtime_pd.get('stack_size', '0'), 0) == 0x200000,
            'runtime startup needs a 2 MiB stack')
    runtime_pd.remove(irq_node)
    channels = runtime_xml.findall('channel')
    require(len(channels) == 1 and [end.attrib for end in channels[0]] == [
        dict(pd='native_startup_preflight', id='7', notify='true', pp='false'),
        dict(pd='native_startup_preflight', id='8', notify='false', pp='false')],
        'exact software self-wakeup channel')
    runtime_xml.remove(channels[0])
    service_owner = root[root.index('fnservice_owner('):root.index('fnnotified(', root.index('fnservice_owner('))]
    require('ifself.router.report().continuation_pending{self.progress_trace.sent();ifletSome(display)=&display{self.paint_progress(display,3);}Channel::new(irq::CONTINUATION_SEND_CHANNELasusize).notify();}' in service_owner,
            'only pending work after the bounded owner loop signals software wakeup')
    require(service_owner.index('forpassin0..qcom_modem_owner::WORK_BUDGET{') <
            service_owner.index('ifself.router.report().continuation_pending{self.progress_trace.sent();'),
            'software wakeup follows the bounded loop')
    require('pubconstCONTINUATION_RECEIVE_CHANNEL:u8=8;' in irq and
            'pubconstCONTINUATION_SEND_CHANNEL:u8=7;' in irq,
            'distinct software wake channels')
    runtime_pd.set('stack_size', auth_pd.get('stack_size'))
    require(tree(runtime_xml) == tree(auth_xml),
            'runtime CapDL system differs only by IRQ481, self-wakeup and audited stack size')

    validator = t['validator']
    for token in ['tftp_service_published=False', 'wlan_firmware_tftp_serving_enabled=False',
                  'ipa_server_endpoint_can_be_announced=True', 'rmtfs_available_before_binding=False',
                  "scope='Offlinepost-authnativeGLINK/QRTRdiscoverycapabilityauditonly;nodeviceresult.'"]:
        require(token in validator, 'truthful capability audit scope: ' + token)
    for forbidden in ['wlan_firmware_tftp_serving_enabled=True',
                      'original_rmtfs_and_tftp_retained=True', 'TFTP/IPAobservercapabilityaudit']:
        require(forbidden not in validator, 'stale observer capability claim: ' + forbidden)

    build = t['build']
    require('mpss-runtime)feature="sunfish-native-startup-mpss-runtime"' in build and
            'validate-native-mpss-runtime.py' in build and 'test-native-mpss-runtime-irq.py' in build,
            'dedicated runtime build and capability validation path')


def load_texts(workspace):
    return {key: (workspace / rel).read_text() for key, rel in PATHS.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workspace', type=Path, default=ROOT)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    workspace = args.workspace.resolve()
    texts = load_texts(workspace)
    check(texts)
    hashes = {rel: sha(workspace / rel) for rel in PATHS.values()}
    result = dict(result='passed', source_sha256=hashes, device_actions=[],
                  tftp_observer_opt_in=True, tftp_publication_not_proven=True, wlan_firmware_tftp_serving_enabled=False,
                  ipa_server_endpoint_can_be_announced=True, remote_storage_available_before_bind=False,
                  physical_glink_ready_proven=False, native_wifi_proven=False,
                  scope='Static source/profile audit only; QRTR discovery and device behavior are not proven.')
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + '\n')
    print('PASS: checked RefCell gate, one-use auth handoff, bounded runtime services, and exact IRQ plus software-wakeup system extension.')


if __name__ == '__main__':
    main()
