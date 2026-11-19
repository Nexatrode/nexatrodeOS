#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
static volatile uint32_t *test_fb;
#define CHECKPOINT_FB test_fb
#include "../platforms/sunfish/loader-boot-checkpoints.h"
#define SUNFISH_LOADER_COPY_INPUTS 1
#include "../platforms/sunfish/loader-copy-inputs.h"
_Noreturn void loader_copy_hold_stop(void) { abort(); }

int main(void)
{
    const size_t prefix = 0x7000, payload = 0xf010, length = 0x46523c4;
    assert(loader_copy_input_errors(0xb0081000, 0xc0000000, prefix, payload, length) == 0);
    assert(loader_copy_input_errors(0x80281000, 0xc0000000, prefix, payload, length) == LOADER_INPUT_SOURCE);
    assert(loader_copy_input_errors(0xb0081000, 0xc1000000, prefix, payload, length) == LOADER_INPUT_DESTINATION);
    assert(loader_copy_input_errors(0xb0081000, 0xc0000000, 0, payload, length) == LOADER_INPUT_EMPTY_PREFIX);
    assert(loader_copy_input_errors(0xb0081000, 0xc0000000, payload + 1, payload, length) == LOADER_INPUT_PREFIX_ORDER);
    assert(loader_copy_input_errors(0xb0081000, 0xc0000000, prefix, length + 1, length) == LOADER_INPUT_PAYLOAD_ORDER);
    assert(loader_copy_input_errors(0xb0081000, 0xc0000000, prefix, payload, 0x5000001) == LOADER_INPUT_TOO_LARGE);
    assert(loader_copy_input_errors(0, 0xc0000000, prefix, payload, length) == (LOADER_INPUT_SOURCE | LOADER_INPUT_NULL));
    assert(loader_copy_input_errors(0xb0081000, 0, prefix, payload, length) == (LOADER_INPUT_DESTINATION | LOADER_INPUT_NULL));
    assert(loader_copy_input_errors(UINTPTR_MAX - 1, 0xc0000000, prefix, payload, length) == (LOADER_INPUT_SOURCE | LOADER_INPUT_SOURCE_OVERFLOW));
    assert(loader_copy_input_errors(0xb0081000, UINTPTR_MAX - 1, prefix, payload, length) == (LOADER_INPUT_DESTINATION | LOADER_INPUT_DEST_OVERFLOW));
    assert(loader_copy_input_errors(0xb0081000, 0xb0081001, prefix, payload, length) == (LOADER_INPUT_DESTINATION | LOADER_INPUT_OVERLAP));
    char rows[9][25];
    loader_copy_input_rows(rows, 0x80281000, 0xc0000000, prefix, payload, length);
    assert(!strcmp(rows[0], "IMAGE COPY INPUTS"));
    assert(!strcmp(rows[1], "SRC 0000000080281000"));
    assert(!strcmp(rows[2], "DST 00000000C0000000"));
    assert(!strcmp(rows[3], "PRE 0000000000007000"));
    assert(!strcmp(rows[4], "PAY 000000000000F010"));
    assert(!strcmp(rows[5], "LEN 00000000046523C4"));
    assert(!strcmp(rows[6], "ERR 0000000000000001"));
    assert(!strcmp(rows[7], "NO COPY OR KERNEL"));
    assert(!strcmp(rows[8], "RESTART WHEN DONE"));
    char text[25];
    loader_copy_input_hex(text, 'M', 'A', 'X', UINT64_MAX);
    assert(!strcmp(text, "MAX FFFFFFFFFFFFFFFF"));
    const unsigned pixels = 1080 * 2340;
    uint32_t *storage = malloc((2 * pixels + 2) * sizeof(uint32_t));
    assert(storage); test_fb = storage + 1;
    for (unsigned i = 0; i < 2 * pixels + 2; i++) storage[i] = 0xa5a5a5a5;
    loader_copy_input_display(rows);
    assert(storage[0] == 0xa5a5a5a5 && storage[2 * pixels + 1] == 0xa5a5a5a5);
    for (unsigned i = 0; i < pixels; i++) {
        assert(test_fb[i] == test_fb[pixels + i]);
        unsigned y = i / 1080;
        if (y < 160 || y >= 1024) assert(test_fb[i] == 0xa5a5a5a5);
        else assert(test_fb[i] == 0x00122b52 || test_fb[i] == 0x0000e5e5);
    }
    /* Foreground and surrounding background for the first and last rows. */
    assert(test_fb[200 * 1080 + 55] == 0x0000e5e5);
    assert(test_fb[(200 + 8 * 96) * 1080 + 48] == 0x0000e5e5);
    assert(test_fb[160 * 1080] == 0x00122b52);
    free(storage);
}
