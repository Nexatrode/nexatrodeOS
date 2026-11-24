/* Disposable offline service: no sockets, peers, credentials or remote relay. */
#include <microkit.h>
#include <stdint.h>
#ifdef SIMPLEX_LIVE_UNIX
static uint64_t started, frequency;
static uint64_t ticks(void) { uint64_t n; asm volatile("mrs %0, cntpct_el0" : "=r"(n)); return n; }
void init(void) { asm volatile("mrs %0, cntfrq_el0" : "=r"(frequency)); started=ticks(); }
#else
void init(void) {}
#endif
void notified(microkit_channel channel) { (void)channel; }
microkit_msginfo protected(microkit_channel channel, microkit_msginfo request)
{
    if ((channel == 0 || channel == 2) && microkit_msginfo_get_label(request) == 0x205 && !microkit_msginfo_get_count(request)) {
        #ifdef SIMPLEX_LIVE_UNIX
        microkit_mr_set(0, SIMPLEX_LIVE_UNIX + (ticks()-started)/frequency);
#else
        microkit_mr_set(0, 1795392000); /* Fixed disposable timestamp. */
#endif
        return microkit_msginfo_new(0x100, 1);
    }
    return microkit_msginfo_new(0x304, 0);
}
