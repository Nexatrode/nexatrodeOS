/* AP image loading only. The generated manifest pins every table field. */
#ifndef SUNFISH_LOADER_TASK_IMAGES_H
#define SUNFISH_LOADER_TASK_IMAGES_H
#include <stdint.h>
#include <stddef.h>
#include "loader-task-manifest.h"

static inline uint32_t loader_task_plan_check(const uint64_t header[8], const uint64_t regions[11][4])
{
    for (unsigned n = 0; n < 8; n++)
        if (header[n] != loader_task_expected_header[n]) return 0xc0000000u | n;
    for (unsigned i = 0; i < 11; i++) for (unsigned n = 0; n < 4; n++)
        if (regions[i][n] != loader_task_expected_regions[i][n]) return 0xc0000100u | (i << 4) | n;
    return UINT32_MAX;
}

#ifdef SUNFISH_LOADER_TASK_IMAGES
_Noreturn void loader_task_images_stop(void);
#ifdef SUNFISH_LOADER_START_KERNEL
void loader_task_images(uintptr_t table);
#else
_Noreturn void loader_task_images(uintptr_t table);
#endif
static _Noreturn void loader_task_failure(uint32_t result)
{
    char text[25] = "IMAGE LOAD FAIL ";
    for (unsigned n = 0; n < 8; n++)
        text[16 + n] = "0123456789ABCDEF"[(result >> (28 - n * 4)) & 15];
    text[24] = 0;
    loader_checkpoint(text);
    loader_task_images_stop();
}
#ifndef SUNFISH_LOADER_START_KERNEL
_Noreturn
#endif
void loader_task_images(uintptr_t table)
{
    /* The table is in our relocated, previously verified loader payload. */
    if ((table & 7) || table < 0xc0000000 || table > 0xc0020000)
        loader_task_failure(0xc0000200);
    const volatile uint64_t *input = (const volatile uint64_t *)table;
    uint64_t header[8], regions[11][4];
    for (unsigned n = 0; n < 8; n++) header[n] = input[n];
    for (unsigned i = 0; i < 11; i++) for (unsigned n = 0; n < 4; n++)
        regions[i][n] = input[8 + i * 4 + n];
    uint32_t result = loader_task_plan_check(header, regions);
    if (result != UINT32_MAX) loader_task_failure(result);
    uintptr_t payload = table + 64 + 11 * 32;
    for (unsigned i = 0; i < 11; i++) {
        char text[25] = "IMAGE LOAD 00";
        text[11] = "0123456789ABCDEF"[i >> 4];
        text[12] = "0123456789ABCDEF"[i & 15];
        loader_checkpoint(text);
        size_t size = regions[i][1];
        const volatile uint8_t *source = (const volatile uint8_t *)(payload + regions[i][2]);
        volatile uint8_t *destination = (volatile uint8_t *)(uintptr_t)regions[i][0];
        result = loader_ram_copy(source, destination, size, size, size);
        if (result == LOADER_RAM_OK) result = loader_ram_verify(source, destination, size, size, size);
        if (result != LOADER_RAM_OK)
            loader_task_failure(result == LOADER_RAM_BOUNDS ? 0xb0000000u | i : (i << 28) | result);
    }
    loader_checkpoint("IMAGE LOAD PASS");
#ifndef SUNFISH_LOADER_START_KERNEL
    loader_task_images_stop();
#endif
}
#endif
#endif
