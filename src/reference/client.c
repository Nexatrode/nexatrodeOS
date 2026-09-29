#include "protocol.h"
#include "crasher_entry.h"

static unsigned int failures;
static unsigned int checks;
static seL4_Bool crash_armed;
static seL4_Bool crash_inflight;
static seL4_Bool crash_request_pending;
static seL4_Word handled_faults;
static seL4_Word counter_before_crash;

static void record(seL4_Bool passed, const char *name)
{
    checks++;
    if (!passed) {
        failures++;
    }
    microkit_dbg_puts(passed ? "PASS: " : "FAIL: ");
    microkit_dbg_puts(name);
    microkit_dbg_puts("\n");
}

static void check(microkit_msginfo reply, seL4_Word label, seL4_Word count,
                  seL4_Word value, const char *name)
{
    record(microkit_msginfo_get_label(reply) == label &&
           microkit_msginfo_get_count(reply) == count &&
           (count == 0 || microkit_mr_get(0) == value), name);
}

void init(void)
{
    microkit_dbg_puts("CLIENT: sending requests through capability channel 0\n");

    microkit_mr_set(0, 19);
    microkit_mr_set(1, 23);
    check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 2)),
          REPLY_OK, 1, 42, "Rust server computes 19 + 23 = 42");

    check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 0)),
          REPLY_BAD_REQUEST, 0, 0, "Rust server rejects missing operands");

    check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(99, 0)),
          REPLY_BAD_REQUEST, 0, 0, "Rust server rejects unknown operation");

    microkit_mr_set(0, ~(seL4_Word)0);
    microkit_mr_set(1, 1);
    check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 2)),
          REPLY_OVERFLOW, 0, 0, "Rust server rejects integer overflow");

    check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_COUNT, 0)),
          REPLY_OK, 1, 1, "rejected requests leave Rust server state unchanged");

    microkit_mr_set(0, 7);
    microkit_mr_set(1, 8);
    check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 2)),
          REPLY_OK, 1, 15, "Rust server responds after malformed requests");

    check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_GUARD, 0)),
          REPLY_OK, 1, SERVER_GUARD_VALUE, "Rust server private page initialized");

    microkit_mr_set(0, 19);
    microkit_mr_set(1, 23);
    check(microkit_ppcall(C_REFERENCE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 2)),
          REPLY_OK, 1, 42, "C reference server answers protected call");

    check(microkit_ppcall(C_REFERENCE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 1)),
          REPLY_BAD_REQUEST, 0, 0, "C reference server rejects wrong message length");

    check(microkit_ppcall(C_REFERENCE_CHANNEL, microkit_msginfo_new(99, 2)),
          REPLY_BAD_REQUEST, 0, 0, "C reference server rejects unknown operation");

    microkit_mr_set(0, ~(seL4_Word)0);
    microkit_mr_set(1, 1);
    check(microkit_ppcall(C_REFERENCE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 2)),
          REPLY_OVERFLOW, 0, 0, "C reference server rejects unsigned overflow");

    if (failures != 0) {
        microkit_dbg_puts("FAIL: baseline failed; fault test not started\n");
        return;
    }
    crash_armed = seL4_True;
    crash_inflight = seL4_True;
    counter_before_crash = 2;
    microkit_dbg_puts("CLIENT: baseline passed; triggering isolated crash\n");
    microkit_notify(START_CRASHER_CHANNEL);
    /* Return and wait. The next phase starts only on a kernel-delivered fault. */
}

static seL4_Bool read_addition_count(seL4_Word *count)
{
    microkit_msginfo reply = microkit_ppcall(
        SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_COUNT, 0));
    if (microkit_msginfo_get_label(reply) != REPLY_OK ||
        microkit_msginfo_get_count(reply) != 1) {
        return seL4_False;
    }
    *count = microkit_mr_get(0);
    return seL4_True;
}

microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    seL4_Word operation = microkit_msginfo_get_label(request);
    seL4_Word count = microkit_msginfo_get_count(request);

    if (channel != CONSOLE_TO_CLIENT_CHANNEL) {
        return microkit_msginfo_new(REPLY_CONSOLE_ERROR, 0);
    }
    if (operation == REQUEST_CONSOLE_STATUS && count == 0) {
        microkit_mr_set(0, handled_faults);
        return microkit_msginfo_new(REPLY_OK, 1);
    }
    if (operation == REQUEST_CONSOLE_CALC && count == 2) {
        seL4_Word left = microkit_mr_get(0);
        seL4_Word right = microkit_mr_get(1);
        microkit_mr_set(0, left);
        microkit_mr_set(1, right);
        microkit_msginfo reply = microkit_ppcall(
            SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 2));
        if (microkit_msginfo_get_label(reply) == REPLY_OK &&
            microkit_msginfo_get_count(reply) == 1) {
            seL4_Word result = microkit_mr_get(0);
            microkit_mr_set(0, result);
            return microkit_msginfo_new(REPLY_OK, 1);
        }
        return reply;
    }
    if (operation == REQUEST_CONSOLE_CRASH && count == 0) {
        if (crash_inflight || crash_request_pending || handled_faults == 0 ||
            !read_addition_count(&counter_before_crash)) {
            return microkit_msginfo_new(REPLY_CONSOLE_ERROR, 0);
        }
        crash_request_pending = seL4_True;
        return microkit_msginfo_new(REPLY_OK, 0);
    }
    return microkit_msginfo_new(REPLY_CONSOLE_ERROR, 0);
}

seL4_Bool fault(microkit_child child, microkit_msginfo message,
                microkit_msginfo *reply_message)
{
    (void)reply_message;
    /* Check message shape before reading fault registers. An unrelated crash,
     * notification, or application message cannot satisfy this condition.
     */
    seL4_Bool expected = crash_armed && crash_inflight && child == CRASHER_CHILD &&
        microkit_msginfo_get_label(message) == seL4_Fault_VMFault &&
        microkit_msginfo_get_count(message) == seL4_VMFault_Length;
    if (expected) {
        seL4_Word syndrome = microkit_mr_get(seL4_VMFault_FSR);
        /* AArch64 ESR: EC=0x24 (data abort from lower EL), WnR=1,
         * DFSC=0b0001xx (translation fault at any translation-table level).
         */
        expected = microkit_mr_get(seL4_VMFault_Addr) == SERVER_PRIVATE_ADDRESS &&
            microkit_mr_get(seL4_VMFault_PrefetchFault) == 0 &&
            ((syndrome >> 26) & 0x3f) == 0x24 &&
            (syndrome & (1ul << 6)) != 0 && (syndrome & 0x3c) == 0x04;
    }
    if (handled_faults == 0) {
        record(expected, "kernel blocked crasher write at 0x40000000");
    } else {
        microkit_dbg_puts(expected ? "PASS: " : "FAIL: ");
        microkit_dbg_puts("kernel blocked crasher write at 0x40000000\n");
        if (!expected) {
            failures++;
        }
    }
    if (!expected) {
        microkit_dbg_puts("FAIL: unexpected child fault; isolation test aborted\n");
        microkit_dbg_puts("FAULT DIAGNOSTIC: child=");
        microkit_dbg_put32(child);
        microkit_dbg_puts(" armed=");
        microkit_dbg_put32(crash_armed);
        microkit_dbg_puts(" inflight=");
        microkit_dbg_put32(crash_inflight);
        microkit_dbg_puts(" label=");
        microkit_dbg_put32(microkit_msginfo_get_label(message));
        microkit_dbg_puts(" words=");
        microkit_dbg_put32(microkit_msginfo_get_count(message));
        if (child == CRASHER_CHILD &&
            microkit_msginfo_get_label(message) == seL4_Fault_VMFault &&
            microkit_msginfo_get_count(message) == seL4_VMFault_Length) {
            microkit_dbg_puts(" address=");
            microkit_dbg_put32(microkit_mr_get(seL4_VMFault_Addr));
            microkit_dbg_puts(" syndrome=");
            microkit_dbg_put32(microkit_mr_get(seL4_VMFault_FSR));
            microkit_dbg_puts(" prefetch=");
            microkit_dbg_put32(microkit_mr_get(seL4_VMFault_PrefetchFault));
        }
        microkit_dbg_puts("\n");
        return seL4_False;
    }
    crash_armed = seL4_False;
    crash_inflight = seL4_False;
    handled_faults++;
    microkit_pd_stop(child);
    microkit_dbg_puts("CLIENT: crasher stopped; checking independent server\n");

    /* Only a checked fault authorizes these post-fault server requests. */
    check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_GUARD, 0)),
          REPLY_OK, 1, SERVER_GUARD_VALUE, "server private page unchanged after fault");
    microkit_msginfo count_reply = microkit_ppcall(
        SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_COUNT, 0));
    if (handled_faults == 1) {
        check(count_reply, REPLY_OK, 1, counter_before_crash,
              "server counter preserved after fault");
    } else {
        seL4_Bool counter_preserved =
            microkit_msginfo_get_label(count_reply) == REPLY_OK &&
            microkit_msginfo_get_count(count_reply) == 1 &&
            microkit_mr_get(0) == counter_before_crash;
        if (!counter_preserved) {
            failures++;
        }
        microkit_dbg_puts(counter_preserved ? "PASS: " : "FAIL: ");
        microkit_dbg_puts("server counter preserved after fault\n");
    }
    microkit_mr_set(0, 100);
    microkit_mr_set(1, 23);
    check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_ADD, 2)),
          REPLY_OK, 1, 123, "independent server computes 100 + 23 after fault");
    if (handled_faults == 1) {
        check(microkit_ppcall(SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_COUNT, 0)),
              REPLY_OK, 1, counter_before_crash + 1,
              "independent server state advances after fault");
        microkit_dbg_puts(failures == 0 && checks == 16
                         ? "ISOLATION COMPLETE: 16 checks passed; crasher stopped, server alive\n"
                         : "FAIL: isolation checks failed\n");
        microkit_notify(CLIENT_TO_CONSOLE_CHANNEL);
    } else {
        microkit_msginfo after_reply = microkit_ppcall(
            SERVICE_CHANNEL, microkit_msginfo_new(REQUEST_COUNT, 0));
        seL4_Bool count_advanced =
            microkit_msginfo_get_label(after_reply) == REPLY_OK &&
            microkit_msginfo_get_count(after_reply) == 1 &&
            microkit_mr_get(0) == counter_before_crash + 1;
        if (!count_advanced) {
            failures++;
        }
        microkit_dbg_puts(count_advanced ? "PASS: " : "FAIL: ");
        microkit_dbg_puts("independent server state advances after fault\n");
        microkit_dbg_puts(failures == 0
                         ? "CRASH COMPLETE: crasher stopped; server remains active\n"
                         : "FAIL: crash recovery checks failed\n");
    }
    /* Do not reply/resume the faulting instruction. No automatic restart. */
    return seL4_False;
}

void notified(microkit_channel channel)
{
    if (channel == CONSOLE_START_CRASHER_CHANNEL &&
        crash_request_pending && !crash_inflight) {
        crash_request_pending = seL4_False;
        crash_armed = seL4_True;
        crash_inflight = seL4_True;
        microkit_pd_restart(CRASHER_CHILD, CRASHER_RESTART_ENTRY);
        microkit_notify(START_CRASHER_CHANNEL);
        return;
    }
    microkit_dbg_puts("FAIL: unexpected client notification\n");
}
