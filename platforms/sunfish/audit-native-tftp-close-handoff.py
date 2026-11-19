#!/usr/bin/env python3
"""Check selected source invariants for the experimental close handoff.

This is not a proof of behavior or hardware safety; host and target tests,
capability review, and boot-layout checks remain separate requirements.
"""
from pathlib import Path
import argparse
import hashlib
import json

parser = argparse.ArgumentParser()
parser.add_argument('--workspace', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--endpoint-observer', action='store_true')
args = parser.parse_args()
root = args.workspace.resolve()
paths = ['rust/src/qcom_glink_qmi.rs', 'rust/src/qcom_glink_fifo.rs',
         'rust/src/qcom_modem_owner.rs', 'rust/src/qcom_tftp_server_check.rs',
         'rust/src/pixel_mpss_runtime.rs']
source = {path: (root / path).read_text() for path in paths}
checks = []

def require(label, condition):
    if not condition:
        raise SystemExit('FAIL: ' + label)
    checks.append(label)

def section(path, start, end):
    text = source[path]
    return text[text.index(start):text.index(end, text.index(start))]

def ordered(text, fragments):
    offsets = [text.index(fragment) for fragment in fragments]
    return offsets == sorted(offsets) and len(set(offsets)) == len(offsets)

session = source[paths[0]]
owner = source[paths[2]]
runtime = source[paths[4]]
require('native prototype remains blocked in the workspace',
        'compile_error!("close-handoff prototype is not yet validated for native boot; host tests only")' in runtime)
require('pending close stops normal session gates',
        'if self.pending_tftp_close.is_some() { return Err(Error::FlowBusy); }' in session)
receive = section(paths[0], 'fn receive_control(', 'fn observe_client_control(')
require('candidate is limited to sole owner-work blocker and valid snapshot',
        'if block_mask == 2 && self.tftp_owner_snapshot_valid' in receive and
        'self.pending_tftp_close = Some((peer, first, index))' in receive)
require('candidate preserves exact WRQ, peer activity, storage-history and fence checks',
        all(x in receive for x in ['matches!(command, control::Command::DelClient(_))',
            '&& self.tftp_server && matching == 1',
            '&rx.bytes[32..32 + request.len()] == request',
            'u32::from(!exact_wrq)', 'u32::from(!owner_clear) << 1',
            'u32::from(other_receive) << 2', 'u32::from(other_transmit) << 3',
            'u32::from(!no_storage_history) << 4', 'u32::from(already_fenced) << 5',
            'u32::from(tombstone.is_none()) << 6']))
commit = section(paths[0], 'pub(crate) fn commit_tftp_close(', 'pub fn retired_rmtfs_peer_count(')
require('commit rechecks exact peer and request, fences and quarantines on error',
        all(x in commit for x in ['if held != peer', 'deferred_rx.state != RxState::Deferred',
            '&deferred_rx.bytes[32..32 + request.len()] != request', 'control::Command::DelClient(peer.port)',
            'self.retired_rmtfs_peers[tombstone] = Some(peer)', 'if result.is_err() { self.quarantined = true; }']))
decision = section(paths[2], 'pub fn resolve_pending_tftp_close(', 'pub fn retired_rmtfs_peer_count(')
require('owner preflight, transport commit, server commit and reply release are ordered',
        ordered(decision, ['self.can_cancel_server_check_reply(peer, server)',
            'self.adapter.commit_tftp_close(self.generation, peer)', 'server.abandon_empty_peer(',
            'self.peer_tftp_replies[slot] = None', 'self.tftp_close_commits =']))
require('owner faults on transport and server commit errors',
        'return Err(self.stop(error.into()))' in decision and 'return Err(self.stop(Error::Stopped))' in decision)
preflight = section(paths[2], 'pub fn can_cancel_server_check_reply(', 'pub fn pending_tftp_close(')
require('owner requires one matching OACK and the exact empty server transfer',
        all(x in preflight for x in ['self.pending_tftp_reply.is_some_and', 'self.tftp_requests.iter().flatten()',
            'replies.next().is_some()', 'reply.meta.route.source.node != 1',
            'reply.bytes[..reply.meta.bytes] != *crate::qcom_tftp_server_check::OACK',
            'server.can_abandon_empty_peer_at(']))
server = source[paths[3]]
require('server abandonment only marks empty transfers and retains port identity',
        'transfer.abandoned || transfer.len != 0 || transfer.last_block != 0 || transfer.complete' in server and
        'transfer.abandoned = true;' in server and 'transfer.peer = None' not in server)
loop = section(paths[4], 'fn service_owner(', '\n    fn notified(')
require('runtime resolves before ordinary services',
        ordered(loop, ['self.router.dispatch(owner)', 'owner.resolve_pending_tftp_close(&mut self.server_check)',
            'self.service_pass(state)?']))
if args.endpoint_observer:
    trace = section(paths[0], 'pub struct TftpEndpointTrace {', '\n}')
    require('endpoint trace holds metadata rather than packet contents',
            all(x in trace for x in ['pub source: Address', 'pub destination: Address',
                'pub payload_bytes: u32', 'pub flags: u32', 'pub remote_intent: u32']) and
            '[u8;' not in trace and 'Vec<' not in trace)
    require('first OACK trace retains distinct queue, publication and completion checkpoints',
            all(x in session for x in ['self.first_tftp_endpoint.is_none() && tftp_packet.starts_with(&[0, 6])',
                'self.first_tftp_slot = Some(index)', 'trace.flags |= 2', 'trace.flags |= 4',
                'trace.flags |= 8', 'self.first_tftp_slot = None']))
    require('endpoint display refreshes without requiring active hardware access',
            'owner.tftp_endpoint_trace() != previous_tftp_endpoint' in runtime and
            'display.hex(23, "OACK FLAGS", trace.flags)' in runtime)
hashes = {path: hashlib.sha256((root / path).read_bytes()).hexdigest() for path in paths}
args.output.write_text(json.dumps({'result': 'pass', 'scope': 'selected static invariants only',
                                   'checks': checks, 'source_sha256': hashes}, indent=2) + '\n')
print(f'PASS: {len(checks)} selected close-handoff source invariants; not a device result.')
