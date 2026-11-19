#include <assert.h>
#include <stdint.h>
#include <string.h>
#include "../platforms/sunfish/loader-task-images.h"
int main(void)
{
    uint64_t header[8], regions[11][4];
    memcpy(header, loader_task_expected_header, sizeof(header));
    memcpy(regions, loader_task_expected_regions, sizeof(regions));
    assert(loader_task_plan_check(header, regions) == UINT32_MAX);
    for (unsigned n = 0; n < 8; n++) {
        header[n] ^= 1;
        assert(loader_task_plan_check(header, regions) == (0xc0000000u | n));
        header[n] ^= 1;
    }
    for (unsigned i = 0; i < 11; i++) for (unsigned n = 0; n < 4; n++) {
        regions[i][n] ^= 1;
        assert(loader_task_plan_check(header, regions) == (0xc0000100u | (i << 4) | n));
        regions[i][n] ^= 1;
    }
    /* A bad final destination must reject the whole plan before earlier
     * valid regions can be copied. */
    regions[10][0] = 0x8b000000;
    assert(loader_task_plan_check(header, regions) == 0xc00001a0);
    assert(memcmp(header, loader_task_expected_header, sizeof(header)) == 0);
}
