#!/usr/bin/env python3
"""Private SDK loader placement for the larger image; never change the base SDK."""
from __future__ import annotations
import argparse
import hashlib
import json
import shutil
import struct
import subprocess
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
BASE=ROOT/'.cache/microkit-src-2.3.1-sunfish/release/microkit-sdk-2.3.1-sunfish'
SOURCE=ROOT/'.cache/microkit-src-2.3.1-sunfish/loader'
# The measured Fastboot Image begins at b1280000. A b0000000
# destination overlaps its body even though the loader code prefixes differ.
LINK=0xc0000000
RUNTIME_SOURCE=0xb1281000

def task_manifest_path(stage_modem_image=False,authenticate_modem=False,observe_modem_ready=False,handshake_modem_status=False,fatal_trace=False,qrtr_servreg=False,qrtr_servreg_pdr_followup=False,glink_open_plan_snapshot=False,glink_open_probe=False,glink_channel_probe=False,glink_intents_probe=False,glink_rx_packet_probe=False,glink_done_probe=False,qrtr_hello_probe=False,qrtr_next_probe=False,qrtr_discovery_probe=False,qrtr_credit_probe=False,qrtr_wait_probe=False,qrtr_servreg_pdr_reply=False,modem_servreg_state_probe=False,modem_servreg_restart_probe=False,modem_tftp_observer=False):
    assert sum([stage_modem_image,authenticate_modem,observe_modem_ready,handshake_modem_status])<=1
    assert not qrtr_servreg or handshake_modem_status
    assert not fatal_trace or handshake_modem_status and not qrtr_servreg
    assert not qrtr_servreg_pdr_followup or qrtr_servreg and handshake_modem_status and not fatal_trace
    assert not qrtr_servreg_pdr_reply or qrtr_servreg and handshake_modem_status and not fatal_trace
    assert not qrtr_servreg_pdr_followup or not qrtr_servreg_pdr_reply
    assert not modem_servreg_state_probe or handshake_modem_status and not qrtr_servreg and not fatal_trace
    assert not modem_servreg_restart_probe or modem_servreg_state_probe and handshake_modem_status and not qrtr_servreg and not fatal_trace
    assert not modem_tftp_observer or modem_servreg_state_probe and handshake_modem_status and not modem_servreg_restart_probe and not qrtr_servreg and not fatal_trace
    assert not glink_open_plan_snapshot or handshake_modem_status and not qrtr_servreg and not fatal_trace
    assert not glink_open_probe or handshake_modem_status and not qrtr_servreg and not fatal_trace and not glink_open_plan_snapshot
    assert not glink_channel_probe or handshake_modem_status and glink_open_probe and not qrtr_servreg and not fatal_trace and not glink_open_plan_snapshot
    assert not glink_intents_probe or handshake_modem_status and glink_channel_probe and not qrtr_servreg and not fatal_trace and not glink_open_plan_snapshot
    assert not glink_rx_packet_probe or handshake_modem_status and glink_intents_probe and glink_channel_probe and not qrtr_servreg and not fatal_trace and not glink_open_plan_snapshot
    assert not glink_done_probe or handshake_modem_status and glink_rx_packet_probe and glink_intents_probe and not qrtr_servreg and not fatal_trace and not glink_open_plan_snapshot
    assert not qrtr_hello_probe or handshake_modem_status and glink_done_probe and not qrtr_servreg and not fatal_trace and not glink_open_plan_snapshot
    assert not qrtr_next_probe or qrtr_hello_probe and handshake_modem_status and not qrtr_servreg and not fatal_trace and not glink_open_plan_snapshot
    assert not qrtr_discovery_probe or qrtr_next_probe and qrtr_hello_probe and handshake_modem_status and not qrtr_servreg and not fatal_trace and not glink_open_plan_snapshot
    assert not qrtr_credit_probe or qrtr_discovery_probe
    assert not qrtr_wait_probe or qrtr_credit_probe
    if fatal_trace:return Path(__file__).with_name('loader-task-regions-fatal-trace.json')
    if glink_open_plan_snapshot:return Path(__file__).with_name('loader-task-regions-glink-plan-snapshot.json')
    if qrtr_wait_probe:return Path(__file__).with_name('loader-task-regions-qrtr-wait.json')
    if modem_servreg_restart_probe:return Path(__file__).with_name('loader-task-regions-modem-servreg-restart.json')
    if modem_tftp_observer:return Path(__file__).with_name('loader-task-regions-modem-tftp-observer.json')
    if modem_servreg_state_probe:return Path(__file__).with_name('loader-task-regions-modem-servreg-state.json')
    if qrtr_credit_probe:return Path(__file__).with_name('loader-task-regions-qrtr-credit.json')
    if qrtr_discovery_probe:return Path(__file__).with_name('loader-task-regions-qrtr-discovery.json')
    if qrtr_next_probe:return Path(__file__).with_name('loader-task-regions-qrtr-next.json')
    if qrtr_hello_probe:return Path(__file__).with_name('loader-task-regions-qrtr-hello.json')
    if glink_done_probe:return Path(__file__).with_name('loader-task-regions-glink-done.json')
    if glink_rx_packet_probe:return Path(__file__).with_name('loader-task-regions-glink-rx-packet.json')
    if glink_intents_probe:return Path(__file__).with_name('loader-task-regions-glink-intents.json')
    if glink_channel_probe:return Path(__file__).with_name('loader-task-regions-glink-channel.json')
    if glink_open_probe:return Path(__file__).with_name('loader-task-regions-glink-open.json')
    if qrtr_servreg_pdr_followup:return Path(__file__).with_name('loader-task-regions-qrtr-pdr-followup.json')
    if qrtr_servreg_pdr_reply:return Path(__file__).with_name('loader-task-regions-qrtr-pdr-reply.json')
    if qrtr_servreg:return Path(__file__).with_name('loader-task-regions-qrtr-servreg.json')
    return Path(__file__).with_name('loader-task-regions-smp2p.json' if handshake_modem_status else ('loader-task-regions-ready.json' if observe_modem_ready else ('loader-task-regions-auth.json' if authenticate_modem else ('loader-task-regions-staging.json' if stage_modem_image else 'loader-task-regions.json'))))

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def native_startup_entry(text, diagnostic_only=False, report_guard=False):
    """Reject unobserved ABL placement before the large-image memmove.

    The pack-time DT audit proves the complete 96 MiB source/destination
    envelopes. This runtime check supplies the missing actual source identity;
    a different bootloader placement stops visibly before relocation.
    """
    anchor='    /* May not have been loaded in the right location.'
    assert text.count(anchor)==1 and '.Lnative_source_ok' not in text
    if diagnostic_only or report_guard:
        if report_guard:
            source_select = f'''    ldr x1, ={hex(RUNTIME_SOURCE)}
    cmp x0, x1
    b.eq .Lnative_source_runtime_ok
    ldr x1, =0x80081000
    cmp x0, x1
    b.ne .Lnative_source_address_fail
    mov x21, #1
    b .Lnative_source_runtime_checks
.Lnative_source_runtime_ok:
    mov x21, #0
.Lnative_source_runtime_checks:
'''
            source_range_check = '''    cbz x21, .Lnative_source_ok
    add x4, x0, x3
    ldr x1, =0x85700000
    cmp x4, x1
    b.hi .Lnative_source_reservation_fail
    b .Lnative_source_ok
'''
            reservation_failure = '''.Lnative_source_reservation_fail:
    mov x1, x4
    adr x0, .Lnative_source_reservation_text
    bl loader_checkpoint_hex
    b .Lnative_source_diagnostic_stop
'''
            failure_exit = '    b .Lnative_source_diagnostic_stop\n'
            reservation_text = '.Lnative_source_reservation_text:\n    .asciz "SOURCE RANGE END"\n'
        else:
            source_select = f'''    ldr x1, ={hex(RUNTIME_SOURCE)}
    cmp x0, x1
    b.ne .Lnative_source_address_fail
'''
            source_range_check = ''
            reservation_failure = ''
            failure_exit = ''
            reservation_text = ''
        guard=f'''    msr daifset, #0xf
    adr x0, _start
    ldr x1, =LINK_ADDRESS
    cmp x0, x1
    b.eq .Lnative_source_link_ok
{source_select}
    adrp x2, _bss_end
    add x2, x2, #:lo12:_bss_end
    sub x3, x2, x0
    ldr x1, =0x20000
    cmp x3, x1
    b.hi .Lnative_source_prefix_fail
    ldr x4, [x2, #8]
    adds x3, x3, x4
    b.cs .Lnative_source_overflow_fail
    ldr x1, =0x6000000
    cmp x3, x1
    b.hi .Lnative_source_extent_fail
{source_range_check}
    b .Lnative_source_ok
.Lnative_source_link_ok:
    adrp x2, _bss_end
    add x2, x2, #:lo12:_bss_end
    sub x3, x2, x0
    ldr x4, [x2, #8]
    add x3, x3, x4
.Lnative_source_ok:
    mov x19, x0
    mov x20, x3
    adr x0, .Lnative_source_address_text
    mov x1, x19
    bl loader_checkpoint_hex
    adr x0, .Lnative_source_extent_text
    mov x1, x20
    bl loader_checkpoint_hex
'''
        if diagnostic_only:
            guard += '    b .Lnative_source_diagnostic_stop\n'
        elif report_guard:
            guard += '    b .Lnative_source_continue\n'
        guard += f'''
.Lnative_source_address_fail:
    mov x1, x0
    adr x0, .Lnative_source_address_text
    bl loader_checkpoint_hex
    b .Lnative_source_diagnostic_stop
.Lnative_source_prefix_fail:
    mov x1, x3
    adr x0, .Lnative_source_prefix_text
    bl loader_checkpoint_hex
    b .Lnative_source_diagnostic_stop
.Lnative_source_overflow_fail:
    mov x1, x3
    adr x0, .Lnative_source_overflow_text
    bl loader_checkpoint_hex
    b .Lnative_source_diagnostic_stop
.Lnative_source_extent_fail:
    mov x1, x3
    adr x0, .Lnative_source_extent_text
    bl loader_checkpoint_hex
{failure_exit}{reservation_failure}
.Lnative_source_diagnostic_stop:
    b .Lnative_source_diagnostic_stop
.Lnative_source_address_text:
    .asciz "SOURCE ADDRESS"
.Lnative_source_prefix_text:
    .asciz "SOURCE PREFIX SIZE"
.Lnative_source_overflow_text:
    .asciz "SOURCE SIZE OVERFLOW"
.Lnative_source_extent_text:
    .asciz "SOURCE EXTENT"
{reservation_text}    .balign 8
'''
        if report_guard:
            guard += '.Lnative_source_continue:\n'
    else:
        guard=f'''    msr daifset, #0xf
    adr x0, _start
    ldr x1, =LINK_ADDRESS
    cmp x0, x1
    b.eq .Lnative_source_ok
    ldr x1, ={hex(RUNTIME_SOURCE)}
    cmp x0, x1
    b.ne .Lnative_source_fail
    adrp x2, _bss_end
    add x2, x2, #:lo12:_bss_end
    sub x3, x2, x0
    ldr x1, =0x20000
    cmp x3, x1
    b.hi .Lnative_source_fail
    ldr x4, [x2, #8]
    adds x3, x3, x4
    b.cs .Lnative_source_fail
    ldr x1, =0x6000000
    cmp x3, x1
    b.hi .Lnative_source_fail
.Lnative_source_ok:
'''
    text=text.replace(anchor,guard+anchor)
    if diagnostic_only or report_guard:
        text+='''
'''
    else:
        text+='''
.Lnative_source_fail:
    adr x0, .Lnative_source_text
    bl loader_checkpoint
.Lnative_source_stop:
    b .Lnative_source_stop
.Lnative_source_text:
    .asciz "IMAGE SOURCE STOP"
    .balign 8
'''
    return text
def segments(p):
    b=p.read_bytes();assert b[:6]==b'\x7fELF\x02\x01'
    off=struct.unpack_from('<Q',b,32)[0];stride,count=struct.unpack_from('<HH',b,54)
    result=[]
    for i in range(count):
        ph=struct.unpack_from('<IIQQQQQQ',b,off+i*stride)
        if ph[0]==1:result.append((ph[3],ph[4],ph[6],b[ph[2]:ph[2]+ph[5]]))
    return result

def board_uart(text):
    """Reproduce the SDK's Sunfish board glue in a private source copy."""
    snippet=Path(__file__).with_name('loader-geni-uart.inc').read_text()
    marker='#elif defined(CONFIG_PLAT_SUNFISH)'
    if marker not in text:
        anchor='#elif defined(CONFIG_PLAT_QEMU_ARM_VIRT)'
        assert text.count(anchor)==1, 'unexpected loader UART source'
        text=text.replace(anchor,snippet+anchor)
    assert text.count(snippet)==1, 'unexpected Sunfish UART implementation'
    return text

def entry_checkpoints(text,hold_after_relocation=False,hold_before_relocation=False,hold_after_copy=False,copy_progress=False,copy_inputs=False,relocation_progress=False,task_images=False):
    assert '.Limage_entry_text' not in text, 'entry checkpoints already applied'
    assert sum([hold_after_relocation,hold_before_relocation,hold_after_copy])<=1, 'choose one hold point'
    assert not copy_progress or hold_after_copy, 'copy progress requires the source copy hold'
    assert not copy_inputs or hold_after_copy, 'copy input snapshot requires the source copy hold'
    assert not (copy_inputs and copy_progress), 'choose snapshot or copy, not both'
    assert not relocation_progress or hold_after_relocation, 'relocation progress requires the destination hold'
    assert not task_images or relocation_progress, 'task image loading requires verified relocation'
    if hold_before_relocation or hold_after_copy or relocation_progress:
        # Match the interrupt masking in the standalone entry that passed on
        # the Pixel. Keep this change scoped to the pre-relocation diagnostic.
        stack_ready='    mov     sp, x1\n'
        assert text.count(stack_ready)==1
        text=text.replace(stack_ready,stack_ready+'    msr daifset, #0xf\n')
    needle='    /* May not have been loaded in the right location.'
    assert text.count(needle)==1
    marker='''    adr x0, .Limage_entry_text
    adr x1, _start
    ldr x2, =LINK_ADDRESS
    cmp x1, x2
    b.ne .Limage_draw_marker
    adr x0, .Limage_relocated_text
.Limage_draw_marker:
    bl loader_checkpoint
'''
    if hold_after_relocation:
        marker+='''    adr x1, _start
    ldr x2, =LINK_ADDRESS
    cmp x1, x2
    b.ne .Limage_continue_relocation
    .global loader_relocated_hold
loader_relocated_hold:
    b loader_relocated_hold
'''
        if relocation_progress:
            marker+='''    .global loader_copy_hold_stop
loader_copy_hold_stop:
    b loader_copy_hold_stop
'''
        marker+='''
.Limage_continue_relocation:
'''
        if task_images:
            marker=marker.replace('loader_relocated_hold:\n    b loader_relocated_hold', '''loader_relocated_hold:
    b main
    .global loader_task_images_stop
loader_task_images_stop:
    b loader_task_images_stop''')
    if hold_before_relocation:
        marker+='''    .global loader_entry_hold
loader_entry_hold:
    b loader_entry_hold
'''
    if hold_after_copy:
        marker+='''    adr x1, _start
    ldr x2, =LINK_ADDRESS
    cmp x1, x2
    b.ne .Limage_copy_origin
    .global loader_copy_already_placed
loader_copy_already_placed:
    b loader_copy_already_placed
    .global loader_copy_hold_stop
loader_copy_hold_stop:
    b loader_copy_hold_stop
.Limage_copy_origin:
'''
    text=text.replace(needle,marker+needle)
    assert text.count('\nrelocate:\n')==1
    text=text.replace('\nrelocate:\n','''
.Limage_entry_text:
    .asciz "IMAGE ENTRY"
.Limage_relocated_text:
    .asciz "IMAGE RELOCATED"
    .balign 4
relocate:
''')
    if hold_after_copy or relocation_progress:
        if hold_after_copy:
            text=text.replace('    .asciz "IMAGE RELOCATED"', '    .asciz "IMAGE COPY SKIP"')
        anchor='    .balign 4\nrelocate:'
        assert text.count(anchor)==1
        text=text.replace(anchor, '.Limage_copy_text:\n    .asciz "IMAGE COPY START"\n'+anchor)
        assert text.count('    bl relocation_log')==1 and text.count('    bl memmove')==1
        text=text.replace('    bl relocation_log','    adr x0, .Limage_copy_text\n    bl loader_checkpoint')
        copy_entry='''    adr x0, _start
    ldr x1, =LINK_ADDRESS
    adrp x2, _bss
    add x2, x2, #:lo12:_bss
    sub x2, x2, x0
    adrp x3, _bss_end
    add x3, x3, #:lo12:_bss_end
    ldr x4, [x3, #8]
    sub x3, x3, x0
    add x4, x4, x3
    b loader_copy_hold'''
        if relocation_progress:
            text=text.replace('    bl memmove',copy_entry.replace('    b loader_copy_hold','    bl loader_copy_progress_resume'))
            assert text.count('    mov x19, x0')==1 and text.count('    bl invalidate_icache')==1
            text=text.replace('    mov x19, x0','''    mov x19, x0
    adr x0, .Limage_cache_text
    bl loader_checkpoint''')
            text=text.replace('    bl invalidate_icache','''    adr x0, .Limage_clean_text
    bl loader_checkpoint
    bl invalidate_icache
    adr x0, .Limage_jump_text
    bl loader_checkpoint''')
            text += '''
.Limage_cache_text:
    .asciz "IMAGE CACHE START"
.Limage_clean_text:
    .asciz "IMAGE CACHE CLEAN"
.Limage_jump_text:
    .asciz "IMAGE JUMP"
    .balign 4
'''
        elif copy_inputs:
            text=text.replace('    bl memmove',copy_entry.replace('b loader_copy_hold','b loader_copy_inputs_hold'))
        elif copy_progress:
            text=text.replace('    bl memmove',copy_entry.replace('b loader_copy_hold','b loader_copy_progress_hold'))
        else:
            text=text.replace('    bl memmove','    bl memmove\n'+copy_entry)
    return text

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--out-dir',type=Path,required=True)
    p.add_argument('--checkpoints',action='store_true')
    p.add_argument('--native-startup-entry',action='store_true',help='Closed large native image with runtime ABL-source guard; no legacy task-copy overlay')
    p.add_argument('--native-source-diagnostic',action='store_true',help='Show the source guard check and stop before relocation on every path')
    p.add_argument('--native-source-report-guard',action='store_true',help='Show labeled source-guard failures; continue to relocation only when all checks pass')
    p.add_argument('--copy-progress',action='store_true')
    p.add_argument('--copy-inputs',action='store_true')
    p.add_argument('--relocation-progress',action='store_true')
    p.add_argument('--task-images',action='store_true')
    continuation=p.add_mutually_exclusive_group()
    continuation.add_argument('--boot-diagnostic',action='store_true')
    continuation.add_argument('--stage-modem-image',action='store_true')
    continuation.add_argument('--authenticate-modem',action='store_true')
    continuation.add_argument('--observe-modem-ready',action='store_true')
    continuation.add_argument('--handshake-modem-status',action='store_true')
    continuation.add_argument('--modem-servreg-state-probe',action='store_true')
    p.add_argument('--modem-servreg-restart-probe',action='store_true')
    p.add_argument('--modem-tftp-observer',action='store_true')
    continuation.add_argument('--qrtr-servreg-probe',action='store_true')
    p.add_argument('--qrtr-servreg-pdr-followup',action='store_true')
    p.add_argument('--qrtr-servreg-pdr-reply',action='store_true')
    p.add_argument('--glink-open-plan-snapshot',action='store_true')
    p.add_argument('--glink-open-probe',action='store_true')
    p.add_argument('--glink-channel-probe',action='store_true')
    p.add_argument('--glink-intents-probe',action='store_true')
    p.add_argument('--glink-rx-packet-probe',action='store_true')
    p.add_argument('--glink-done-probe',action='store_true')
    p.add_argument('--qrtr-hello-probe',action='store_true')
    p.add_argument('--qrtr-next-probe',action='store_true')
    p.add_argument('--qrtr-discovery-probe',action='store_true')
    p.add_argument('--qrtr-credit-probe',action='store_true')
    p.add_argument('--qrtr-wait-probe',action='store_true')
    p.add_argument('--fatal-trace',action='store_true')
    holds=p.add_mutually_exclusive_group()
    holds.add_argument('--hold-after-relocation',action='store_true')
    holds.add_argument('--hold-before-relocation',action='store_true')
    holds.add_argument('--hold-after-copy',action='store_true');args=p.parse_args()
    guard_modes = (args.native_startup_entry, args.native_source_diagnostic,
                   args.native_source_report_guard)
    if sum(guard_modes) > 1:
        p.error('select only one native source-guard mode')
    if any(guard_modes) and (not args.checkpoints or any(
        value for key,value in vars(args).items()
        if isinstance(value,bool) and key not in ['native_startup_entry','native_source_diagnostic','native_source_report_guard','checkpoints'])):
        p.error('native source guard requires --checkpoints and excludes diagnostic/legacy task overlays')
    if (args.hold_after_relocation or args.hold_before_relocation or args.hold_after_copy) and not args.checkpoints:
        p.error('a loader hold requires --checkpoints')
    if args.copy_progress and not args.hold_after_copy:
        p.error('--copy-progress requires --hold-after-copy')
    if args.copy_inputs and (not args.hold_after_copy or args.copy_progress):
        p.error('--copy-inputs requires --hold-after-copy and excludes --copy-progress')
    if args.relocation_progress and not args.hold_after_relocation:
        p.error('--relocation-progress requires --hold-after-relocation')
    if args.task_images and not args.relocation_progress:
        p.error('--task-images requires --relocation-progress')
    if args.boot_diagnostic and not args.task_images:
        p.error('--boot-diagnostic requires --task-images')
    if args.stage_modem_image and not args.task_images:
        p.error('--stage-modem-image requires --task-images')
    if args.authenticate_modem and not args.task_images:
        p.error('--authenticate-modem requires --task-images')
    if (args.handshake_modem_status or args.modem_servreg_state_probe or args.modem_servreg_restart_probe or args.modem_tftp_observer or args.qrtr_servreg_probe or args.qrtr_servreg_pdr_followup) and not args.task_images:
        p.error('--handshake-modem-status requires --task-images')
    if args.modem_tftp_observer and (not args.modem_servreg_state_probe or args.modem_servreg_restart_probe):
        p.error('--modem-tftp-observer requires --modem-servreg-state-probe and excludes restart')
    if args.qrtr_servreg_pdr_followup and not args.qrtr_servreg_probe:
        p.error('--qrtr-servreg-pdr-followup requires --qrtr-servreg-probe')
    if args.qrtr_servreg_pdr_reply and (not args.qrtr_servreg_probe or args.qrtr_servreg_pdr_followup):
        p.error('--qrtr-servreg-pdr-reply requires --qrtr-servreg-probe and excludes --qrtr-servreg-pdr-followup')
    if args.glink_open_plan_snapshot and (not args.handshake_modem_status or args.qrtr_servreg_probe or args.fatal_trace):
        p.error('--glink-open-plan-snapshot requires a plain SMP2P handshake')
    if args.glink_open_probe and (not args.handshake_modem_status or args.qrtr_servreg_probe or args.fatal_trace or args.glink_open_plan_snapshot):
        p.error('--glink-open-probe requires a plain SMP2P handshake and excludes snapshot/QRTR/fatal-trace modes')
    if args.glink_channel_probe and (not args.glink_open_probe or not args.handshake_modem_status or args.qrtr_servreg_probe or args.fatal_trace or args.glink_open_plan_snapshot):
        p.error('--glink-channel-probe requires a plain handshake plus OPEN and excludes snapshot/QRTR/fatal-trace modes')
    if args.glink_intents_probe and (not args.glink_channel_probe or not args.glink_open_probe or not args.handshake_modem_status or args.qrtr_servreg_probe or args.fatal_trace or args.glink_open_plan_snapshot):
        p.error('--glink-intents-probe requires a plain handshake plus OPEN/channel and excludes snapshot/QRTR/fatal-trace modes')
    if args.glink_rx_packet_probe and (not args.glink_intents_probe or not args.glink_channel_probe or not args.handshake_modem_status or args.qrtr_servreg_probe or args.fatal_trace or args.glink_open_plan_snapshot):
        p.error('--glink-rx-packet-probe requires a plain handshake plus OPEN/channel/intent inventory and excludes snapshot/QRTR/fatal-trace modes')
    if args.glink_done_probe and (not args.glink_rx_packet_probe or not args.glink_intents_probe or not args.handshake_modem_status or args.qrtr_servreg_probe or args.fatal_trace or args.glink_open_plan_snapshot):
        p.error('--glink-done-probe requires a plain handshake plus OPEN/channel/intent/receive probes and excludes snapshot/QRTR/fatal-trace modes')
    if args.qrtr_hello_probe and (not args.glink_done_probe or not args.handshake_modem_status or args.qrtr_servreg_probe or args.fatal_trace or args.glink_open_plan_snapshot):
        p.error('--qrtr-hello-probe requires a plain handshake plus GLINK receive completion and excludes snapshot/QRTR-service/fatal-trace modes')
    if args.qrtr_next_probe and (not args.qrtr_hello_probe or not args.handshake_modem_status or args.qrtr_servreg_probe or args.fatal_trace or args.glink_open_plan_snapshot):
        p.error('--qrtr-next-probe requires the bounded HELLO probe and excludes snapshot/QRTR-service/fatal-trace modes')
    if args.qrtr_discovery_probe and (not args.qrtr_next_probe or not args.qrtr_hello_probe or not args.handshake_modem_status or args.qrtr_servreg_probe or args.fatal_trace or args.glink_open_plan_snapshot):
        p.error('--qrtr-discovery-probe requires the bounded HELLO/next-packet probes and excludes snapshot/QRTR-service/fatal-trace modes')
    if args.qrtr_credit_probe and not args.qrtr_discovery_probe:
        p.error('--qrtr-credit-probe requires --qrtr-discovery-probe')
    if args.qrtr_wait_probe and not args.qrtr_credit_probe:
        p.error('--qrtr-wait-probe requires --qrtr-credit-probe')
    if args.fatal_trace and not args.handshake_modem_status:
        p.error('--fatal-trace requires --handshake-modem-status')
    if args.observe_modem_ready and not args.task_images:
        p.error('--observe-modem-ready requires --task-images')
    out=args.out_dir.resolve();sdk=out/'sdk';out.mkdir(parents=True,exist_ok=True)
    original=BASE/'board/sunfish/debug/elf/loader.elf';original_hash=sha(original)
    shutil.copytree(BASE,sdk,dirs_exist_ok=True)
    board_source=out/'loader-board-source'
    shutil.copytree(SOURCE,board_source,dirs_exist_ok=True)
    uart=board_source/'src/uart.c';uart.write_text(board_uart(uart.read_text()))
    def build(name,address,source=board_source):
        # A stale object can match an old SDK while ignoring today's source.
        # Rebuild every object in these disposable, task-owned directories.
        build_dir=out/name;build_dir.mkdir(exist_ok=True)
        shutil.rmtree(build_dir);build_dir.mkdir()
        subprocess.run(['make','-C',str(source),'all','BUILD_DIR='+str(build_dir),'ARCH=aarch64','BOARD=sunfish',
                        'LINK_ADDRESS='+hex(address),'SEL4_SDK='+str(BASE/'board/sunfish/debug'),
                        'TARGET_TRIPLE=aarch64-none-elf','LLVM=True','GCC_CPU=cortex-a55'],check=True)
        return build_dir/'loader.elf'
    baseline=build('loader-baseline-build',0x81000000)
    assert segments(baseline)==segments(original),'loader source/toolchain does not reproduce installed loadable segments'
    variant_source=board_source
    if args.checkpoints:
        variant_source=out/'loader-checkpoint-source'
        shutil.copytree(board_source,variant_source,dirs_exist_ok=True)
        shutil.copy2(Path(__file__).with_name('loader-boot-checkpoints.h'),variant_source/'src/loader-boot-checkpoints.h')
        crt=variant_source/'src/aarch64/crt0.S'
        entry=entry_checkpoints(crt.read_text(),args.hold_after_relocation,args.hold_before_relocation,args.hold_after_copy,args.copy_progress,args.copy_inputs,args.relocation_progress,args.task_images)
        if any(guard_modes):
            entry=native_startup_entry(
                entry,
                diagnostic_only=args.native_source_diagnostic,
                report_guard=args.native_source_report_guard,
            )
        crt.write_text(entry)
        c=variant_source/'src/loader.c';text=c.read_text()
        text='#include "loader-boot-checkpoints.h"\n'+text
        if args.hold_after_copy or args.relocation_progress:
            header='loader-copy-inputs.h' if args.copy_inputs else ('loader-copy-progress.h' if args.copy_progress or args.relocation_progress else 'loader-copy-check.h')
            define='SUNFISH_LOADER_COPY_INPUTS' if args.copy_inputs else ('SUNFISH_LOADER_COPY_PROGRESS' if args.copy_progress or args.relocation_progress else 'SUNFISH_LOADER_COPY_HOLD')
            shutil.copy2(Path(__file__).with_name(header),variant_source/'src'/header)
            extra='#define SUNFISH_LOADER_COPY_CONTINUE 1\n' if args.relocation_progress else ''
            text=text.replace('#include "loader-boot-checkpoints.h"', '#include "loader-boot-checkpoints.h"\n#define '+define+' 1\n'+extra+'#include "'+header+'"')
        if args.task_images:
            arch_init=variant_source/'src/aarch64/init.c';arch_text=arch_init.read_text()
            anchor='#if !defined(ARM_PSCI_UNAVAILABLE)';assert arch_text.count(anchor)==1
            arch_init.write_text(arch_text.replace(anchor,'#if 0 /* No firmware query in this RAM-only diagnostic. */'))
            uart_header=variant_source/'src/uart.h';uart_text=uart_header.read_text()
            anchor='#include <kernel/gen_config.h>\n';assert uart_text.count(anchor)==1
            uart_header.write_text(uart_text.replace(anchor,anchor+'#undef CONFIG_PRINTING /* Visible-only task image diagnostic. */\n'))
            linker=variant_source/'aarch64.ld';linker_text=linker.read_text()
            assert linker_text.count('        . = ALIGN(4);')==1
            linker.write_text(linker_text.replace('        . = ALIGN(4);','        . = ALIGN(8);'))
            handshake=args.handshake_modem_status or args.modem_servreg_state_probe or args.modem_servreg_restart_probe or args.qrtr_servreg_probe or args.qrtr_servreg_pdr_followup or args.qrtr_hello_probe or args.qrtr_next_probe
            manifest=json.loads(task_manifest_path(args.stage_modem_image,args.authenticate_modem,args.observe_modem_ready,handshake,args.fatal_trace,args.qrtr_servreg_probe,args.qrtr_servreg_pdr_followup,args.glink_open_plan_snapshot,args.glink_open_probe,args.glink_channel_probe,args.glink_intents_probe,args.glink_rx_packet_probe,args.glink_done_probe,args.qrtr_hello_probe,args.qrtr_next_probe,args.qrtr_discovery_probe,args.qrtr_credit_probe,args.qrtr_wait_probe,args.qrtr_servreg_pdr_reply,args.modem_servreg_state_probe or args.modem_servreg_restart_probe,args.modem_servreg_restart_probe,args.modem_tftp_observer).read_text())
            assert len(manifest['header'])==8 and len(manifest['regions'])==11
            generated='#include <stdint.h>\nstatic const uint64_t loader_task_expected_header[8] = {'+','.join(v+'ull' for v in manifest['header'])+'};\n'
            generated+='static const uint64_t loader_task_expected_regions[11][4] = {\n'+''.join('    {'+','.join(v+'ull' for v in row)+'},\n' for row in manifest['regions'])+'};\n'
            (variant_source/'src/loader-task-manifest.h').write_text(generated)
            shutil.copy2(Path(__file__).with_name('loader-task-images.h'),variant_source/'src/loader-task-images.h')
            text='#define SUNFISH_LOADER_TASK_IMAGES 1\n'+text
            if args.boot_diagnostic or args.stage_modem_image or args.authenticate_modem or args.observe_modem_ready or args.handshake_modem_status or args.modem_servreg_state_probe or args.modem_servreg_restart_probe or args.modem_tftp_observer or args.qrtr_servreg_probe or args.qrtr_servreg_pdr_followup:
                text='#define SUNFISH_LOADER_START_KERNEL 1\n'+text
                config=(BASE/'board/sunfish/debug/include/kernel/gen_config.h').read_text()
                assert '#define CONFIG_MAX_NUM_NODES  1\n' in config, 'diagnostic must use one CPU'
                text=text.replace('_Static_assert(sizeof(uintptr_t)', '_Static_assert(NUM_ACTIVE_CPUS == 1, "diagnostic cannot start secondary CPUs");\n\n_Static_assert(sizeof(uintptr_t)')
            text=text.replace('#include "loader-copy-progress.h"','#include "loader-copy-progress.h"\n#include "loader-task-images.h"')
            start=text.index('static void copy_data(void)\n{');end=text.index('\n#ifdef CONFIG_PRINTING',start)
            text=text[:start]+'''static void copy_data(void)
{
    loader_task_images((uintptr_t)loader_data);
}
'''+text[end:]
            text=text.replace('    uart_init();\n    /* After any UART','    loader_checkpoint("IMAGE MAIN");\n    uart_init();\n    /* After any UART')
        assert text.count('    copy_data();')==1 and text.count('    start_kernel(0);')==1
        text=text.replace('    copy_data();','    loader_checkpoint("IMAGE COPYING");\n    copy_data();\n    loader_checkpoint("IMAGE COPIED");')
        text=text.replace('    start_kernel(0);','    loader_checkpoint("IMAGE KERNEL");\n    start_kernel(0);');c.write_text(text)
    relocated=build('loader-relocated-build',LINK,variant_source)
    assert segments(relocated)[0][0:2]==(LINK,LINK)
    shutil.copy2(relocated,sdk/'board/sunfish/debug/elf/loader.elf')
    for name in ['sel4.elf','initialiser.elf','monitor.elf']:
        assert sha(sdk/'board/sunfish/debug/elf'/name)==sha(BASE/'board/sunfish/debug/elf'/name)
    assert sha(original)==original_hash
    record={'loader_link_address':hex(LINK),'private_sdk':str(sdk.relative_to(ROOT)),
            'base_sdk_loader_sha256':original_hash,'rebuilt_baseline_loadable_segments_match':True,
            'relocated_loader_sha256':sha(relocated),'base_sdk_unchanged':True,
            'kernel_initialiser_monitor_unchanged':True,
            'visible_checkpoint_overlay':args.checkpoints,
            'native_startup_entry_guard':any(guard_modes),
            'native_source_diagnostic':args.native_source_diagnostic,
            'native_source_report_guard':args.native_source_report_guard,
            'native_startup_max_relocation_bytes':0x6000000 if any(guard_modes) else None,
            'hold_after_relocation':args.hold_after_relocation,
            'hold_before_relocation':args.hold_before_relocation,
            'hold_after_copy':args.hold_after_copy,
            'copy_progress':args.copy_progress,
            'copy_inputs':args.copy_inputs,
            'relocation_progress':args.relocation_progress,
            'task_images':args.task_images,
            'boot_diagnostic':args.boot_diagnostic,
            'stage_modem_image':args.stage_modem_image,
            'authenticate_modem':args.authenticate_modem,
            'observe_modem_ready':args.observe_modem_ready,
            'handshake_modem_status':args.handshake_modem_status,
            'modem_servreg_state_probe':args.modem_servreg_state_probe,
            'modem_servreg_restart_probe':args.modem_servreg_restart_probe,
            'modem_tftp_observer':args.modem_tftp_observer,
            'qrtr_servreg_probe':args.qrtr_servreg_probe,
            'qrtr_servreg_pdr_followup':args.qrtr_servreg_pdr_followup,
            'qrtr_servreg_pdr_reply':args.qrtr_servreg_pdr_reply,
            'glink_open_plan_snapshot':args.glink_open_plan_snapshot,
            'glink_open_probe':args.glink_open_probe,
            'glink_channel_probe':args.glink_channel_probe,
            'glink_intents_probe':args.glink_intents_probe,
            'glink_rx_packet_probe':args.glink_rx_packet_probe,
            'glink_done_probe':args.glink_done_probe,
            'qrtr_hello_probe':args.qrtr_hello_probe,
            'qrtr_next_probe':args.qrtr_next_probe,
            'qrtr_discovery_probe':args.qrtr_discovery_probe,
            'qrtr_credit_probe':args.qrtr_credit_probe,
            'qrtr_wait_probe':args.qrtr_wait_probe,
            'fatal_trace':args.fatal_trace,
            'kernel_start':args.boot_diagnostic or args.stage_modem_image or args.authenticate_modem or args.observe_modem_ready or args.handshake_modem_status or args.modem_servreg_state_probe or args.modem_servreg_restart_probe or args.modem_tftp_observer or args.qrtr_servreg_probe or args.qrtr_servreg_pdr_followup or args.glink_open_probe,
            'task_manifest':str(task_manifest_path(args.stage_modem_image,args.authenticate_modem,args.observe_modem_ready,args.handshake_modem_status or args.modem_servreg_state_probe or args.modem_servreg_restart_probe or args.qrtr_servreg_probe or args.qrtr_servreg_pdr_followup or args.glink_open_probe or args.qrtr_hello_probe or args.qrtr_next_probe or args.qrtr_discovery_probe,args.fatal_trace,args.qrtr_servreg_probe,args.qrtr_servreg_pdr_followup,args.glink_open_plan_snapshot,args.glink_open_probe,args.glink_channel_probe,args.glink_intents_probe,args.glink_rx_packet_probe,args.glink_done_probe,args.qrtr_hello_probe,args.qrtr_next_probe,args.qrtr_discovery_probe,args.qrtr_credit_probe,args.qrtr_wait_probe,args.qrtr_servreg_pdr_reply,args.modem_servreg_state_probe or args.modem_servreg_restart_probe,args.modem_servreg_restart_probe,args.modem_tftp_observer).relative_to(ROOT)) if args.task_images else None,
            'task_manifest_sha256':sha(task_manifest_path(args.stage_modem_image,args.authenticate_modem,args.observe_modem_ready,args.handshake_modem_status or args.modem_servreg_state_probe or args.modem_servreg_restart_probe or args.qrtr_servreg_probe or args.qrtr_servreg_pdr_followup or args.glink_open_probe or args.qrtr_hello_probe or args.qrtr_next_probe or args.qrtr_discovery_probe,args.fatal_trace,args.qrtr_servreg_probe,args.qrtr_servreg_pdr_followup,args.glink_open_plan_snapshot,args.glink_open_probe,args.glink_channel_probe,args.glink_intents_probe,args.glink_rx_packet_probe,args.glink_done_probe,args.qrtr_hello_probe,args.qrtr_next_probe,args.qrtr_discovery_probe,args.qrtr_credit_probe,args.qrtr_wait_probe,args.qrtr_servreg_pdr_reply,args.modem_servreg_state_probe or args.modem_servreg_restart_probe,args.modem_servreg_restart_probe,args.modem_tftp_observer)) if args.task_images else None,
            'uart_output_suppressed':args.task_images,
            'psci_version_query_skipped':args.task_images,
            'expected_runtime_source':hex(RUNTIME_SOURCE),
            'source_image_base':hex(RUNTIME_SOURCE-4096),
            'entry_interrupt_masked':any(guard_modes) or args.hold_before_relocation or args.hold_after_copy or args.relocation_progress,
            'all_objects_rebuilt_from_private_board_source':True,
            'board_uart_source_sha256':sha(board_source/'src/uart.c'),
            'scope':'Only the isolated image build uses this loader. Full relocated payload range must pass DT/target exclusion checks before temporary boot.'}
    (out/'loader-sdk-profile.json').write_text(json.dumps(record,indent=2)+'\n')
    print('Prepared private SDK loader at',hex(LINK))

if __name__=='__main__':main()
