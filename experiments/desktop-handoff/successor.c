#include "app.h"
__attribute__((section(".entry"))) const char *run(const struct app_api *api, size_t image_bytes)
{
    const volatile uint8_t *ram = (const volatile uint8_t *)RAM_ADDR;
    const volatile uint8_t *image = (const volatile uint8_t *)IMAGE_ADDR;
    for (size_t i = 0; i < RAM_BYTES; ++i)
        if (ram[i])
            return "old scratch bytes";
    for (size_t i = image_bytes; i < IMAGE_BYTES; ++i)
        if (image[i])
            return "old executable bytes";
    api->log("HANDOFF|PASS|reused scratch and code tail clean\n");
    if (api->cap_present(CHESS_PORT)) {
        if (api->reopen(CHESS_PORT, TEST_GAME, sizeof(TEST_GAME) - 1))
            return "old chess capability reopened previous game";
        return "unexpected capability at chess port";
    }
    if (!api->cap_present(OTHER_PORT) || !api->empty(OTHER_PORT))
        return "successor namespace";
    if (!api->wallet_read_denied(OTHER_PORT))
        return "wallet read through successor channel";
    static const char fresh[] = "separate save";
    if (!api->save(OTHER_PORT, fresh, sizeof(fresh) - 1) ||
        !api->reopen(OTHER_PORT, fresh, sizeof(fresh) - 1))
        return "successor own storage";
    api->log("HANDOFF|PASS|old storage capability absent and new storage independent\n");
    return 0;
}
