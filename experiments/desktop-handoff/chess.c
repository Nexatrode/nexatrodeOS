#include "app.h"
static const char moves[] = TEST_GAME;

/* Ensure some old executable-page contents really would be visible beyond
 * the successor's shorter image if the manager only copied the new bytes. */
__attribute__((used, section(".tail"))) static const uint8_t tail = 0xcb;

__attribute__((section(".entry"))) const char *run(const struct app_api *api, size_t image_bytes)
{
    (void)image_bytes;
    if (!api->cap_present(CHESS_PORT) || !api->empty(CHESS_PORT))
        return "initial chess storage";
    if (!api->save(CHESS_PORT, moves, sizeof(moves) - 1) ||
        !api->reopen(CHESS_PORT, moves, sizeof(moves) - 1))
        return "chess save and reopen";
    api->log("HANDOFF|PASS|chess saved and reopened game\n");
    if (!api->wallet_read_denied(CHESS_PORT))
        return "wallet read through chess channel";
    api->log("HANDOFF|PASS|wallet file request denied to chess\n");
    volatile uint8_t *ram = (volatile uint8_t *)RAM_ADDR;
    for (size_t i = 0; i < RAM_BYTES; ++i)
        ram[i] = 0xa5;
    return 0;
}
