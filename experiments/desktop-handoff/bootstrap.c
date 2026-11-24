#include "app.h"
#include <microkit.h>

uintptr_t store_page;
struct transfer {
    uint32_t length;
    uint8_t bytes[4092];
};
static volatile struct transfer *page(void) { return (volatile struct transfer *)store_page; }

static int empty(unsigned port)
{
    microkit_msginfo r = microkit_ppcall(port, microkit_msginfo_new(0x30b, 0));
    return microkit_msginfo_get_label(r) == 0x30d && microkit_msginfo_get_count(r) == 0;
}
static int save(unsigned port, const char *bytes, size_t len)
{
    if (len > sizeof(page()->bytes))
        return 0;
    for (size_t i = 0; i < len; ++i)
        page()->bytes[i] = bytes[i];
    page()->length = len;
    microkit_mr_set(0, 0); /* create in empty namespace */
    microkit_mr_set(1, len);
    microkit_msginfo r = microkit_ppcall(port, microkit_msginfo_new(0x30c, 2));
    return microkit_msginfo_get_label(r) == 0x300 && microkit_msginfo_get_count(r) == 1 &&
           microkit_mr_get(0) == 1;
}
static int reopen(unsigned port, const char *bytes, size_t len)
{
    /* Poison the whole transfer buffer before the readback. */
    for (size_t i = 0; i < sizeof(page()->bytes); ++i)
        page()->bytes[i] = 0xde;
    page()->length = 0;
    microkit_msginfo r = microkit_ppcall(port, microkit_msginfo_new(0x30b, 0));
    if (microkit_msginfo_get_label(r) != 0x300 || microkit_msginfo_get_count(r) != 2 ||
        microkit_mr_get(0) != 1 || microkit_mr_get(1) != len || page()->length != len)
        return 0;
    for (size_t i = 0; i < len; ++i)
        if (page()->bytes[i] != (uint8_t)bytes[i])
            return 0;
    return 1;
}
static int wallet_read_denied(unsigned port)
{
    /* Identical wallet file ID, offset and length to the authorized wallet read.
     * A self-declared name in the transfer page cannot affect caller identity. */
    const char name[] = "NexaWal";
    for (size_t i = 0; i < sizeof(name); ++i)
        page()->bytes[i] = name[i];
    page()->length = sizeof(name);
    microkit_mr_set(0, 2);
    microkit_mr_set(1, 0);
    microkit_mr_set(2, sizeof("disposable wallet state") - 1);
    microkit_msginfo r = microkit_ppcall(port, microkit_msginfo_new(0x430, 3));
    unsigned label = microkit_msginfo_get_label(r);
    return (label == 0x303 || label == 0x304) && microkit_msginfo_get_count(r) == 0;
}
static int cap_present(unsigned port)
{
    return seL4_DebugCapIdentify(BASE_ENDPOINT_CAP + port) != 0;
}
static const struct app_api api = {
    .save = save,
    .reopen = reopen,
    .empty = empty,
    .wallet_read_denied = wallet_read_denied,
    .cap_present = cap_present,
    .log = microkit_dbg_puts,
};

void init(void) {}
void notified(microkit_channel ch)
{
    if (ch != 0) {
        microkit_dbg_puts("HANDOFF|FAIL|unexpected launch channel\n");
        return;
    }
    /* The supervisor writes the length into a private read-only launch page. */
    const size_t length = *(const volatile uint32_t *)0x35000000UL;
    const char *error = ((app_entry)IMAGE_ADDR)(&api, length);
    if (error) {
        microkit_dbg_puts("HANDOFF|FAIL|");
        microkit_dbg_puts(error);
        microkit_dbg_puts("\n");
        return;
    }
#ifdef CONTROL_KEEP_IDENTITY
    /* The control parks the old bootstrap so the supervisor can launch
     * different code with the original capability space. */
    microkit_notify(0);
    return;
#endif
    /* Normal load under MMU protection. The supervisor checks the exact fault
     * and only then advances the experiment. */
    uintptr_t private_address = store_page == CHESS_PAGE ? WALLET_PAGE : CHESS_PAGE;
    (void)*(volatile uint8_t *)private_address;
    microkit_dbg_puts("HANDOFF|FAIL|foreign transfer page readable\n");
}
