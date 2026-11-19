#include <microkit.h>
#include <stdint.h>

uintptr_t shared_addr;

/* One independently written cache line per PD in the shared 4 KiB page. */
struct lane {
    uint64_t ticks;
    uint8_t padding[56];
};
_Static_assert(sizeof(struct lane) == 64, "cache line size");

static uint64_t counter(void)
{
    uint64_t ticks;
    __asm__ volatile("isb; mrs %0, cntpct_el0" : "=r"(ticks));
    return ticks;
}

static uint64_t frequency(void)
{
    uint64_t hz;
    __asm__ volatile("mrs %0, cntfrq_el0" : "=r"(hz));
    return hz;
}

static void decimal(uint64_t number)
{
    char buf[21];
    unsigned pos = sizeof(buf);
    buf[--pos] = '\0';
    do {
        buf[--pos] = '0' + number % 10;
        number /= 10;
    } while (number);
    microkit_dbg_puts(&buf[pos]);
}

static void report_progress(struct lane *lanes)
{
    uint64_t before[NCPU];
    for (unsigned i = 1; i < NCPU; ++i) {
        do {
            before[i] = __atomic_load_n(&lanes[i].ticks, __ATOMIC_ACQUIRE);
        } while (before[i] == 0);
    }
    uint64_t start = counter();
    uint64_t interval = frequency() / 20;
    while (counter() - start < interval) {}
    microkit_dbg_puts("SMP|READY cores=");
    decimal(NCPU);
    for (unsigned i = 1; i < NCPU; ++i) {
        uint64_t after = __atomic_load_n(&lanes[i].ticks, __ATOMIC_ACQUIRE);
        microkit_dbg_puts(" delta");
        decimal(i);
        microkit_dbg_puts("=");
        decimal(after - before[i]);
    }
    microkit_dbg_puts(" frequency=");
    decimal(frequency());
    microkit_dbg_puts("\n");
}

void init(void)
{
    struct lane *lanes = (struct lane *)shared_addr;
    __atomic_store_n(&lanes[CPU].ticks, counter(), __ATOMIC_RELEASE);
    if (CPU == 0) {
        report_progress(lanes);
    }
    uint64_t tag = UINT64_C(0x534d5000) + CPU;
    /* x25 identifies the PD in a paused QMP register capture on each vCPU. */
    __asm__ volatile("mov x25,%0\n"
                     "mov x24,%1\n"
                     "1: mrs x26,cntpct_el0\n"
                     "stlr x26,[x24]\n"
                     "b 1b\n"
                     :: "r"(tag), "r"(&lanes[CPU].ticks) : "x24", "x25", "x26", "memory");
    __builtin_unreachable();
}

void notified(microkit_channel ch)
{
    (void)ch;
}
