#include <pongo.h>

#define PAGE_SIZE_16K 0x4000ULL
#define PAGE_REFBITS 0x00ffffffu

extern uint32_t phys_get_entry(uint64_t pa);
extern int iprintf(const char *format, ...);

static void freemap_cmd(const char *cmd, char *args)
{
    (void)cmd;
    (void)args;
    uint64_t base = gBootArgs->physBase;
    uint64_t end = base + gBootArgs->memSize;
    uint64_t run_start = 0;
    uint64_t pages = 0;

    iprintf("freemap: physBase=0x%llx memSize=0x%llx\n",
            base, gBootArgs->memSize);
    for (uint64_t pa = base; pa < end; pa += PAGE_SIZE_16K) {
        if ((phys_get_entry(pa) & PAGE_REFBITS) == 0) {
            if (run_start == 0) run_start = pa;
            pages++;
        } else if (run_start != 0) {
            iprintf("FREE 0x%llx 0x%llx pages=%llu\n",
                    run_start, pa, pages);
            run_start = 0;
            pages = 0;
        }
    }
    if (run_start != 0)
        iprintf("FREE 0x%llx 0x%llx pages=%llu\n",
                run_start, end, pages);
}

void module_entry(void)
{
    command_register("freemap", "prints Pongo allocator free ranges", freemap_cmd);
}

char *module_name = "iphone7plus_freemap";
struct pongo_exports exported_symbols[] = { { .name = 0, .value = 0 } };
