/* SPDX-License-Identifier: GPL-2.0+ */
/* Included in the private comparison U-Boot board build only. */
#include <fdtdec.h>

static u64 comparison_initrd_property(const void *fdt, int chosen,
				     const char *name)
{
	u64 value = fdtdec_get_uint64(fdt, chosen, name, 0);

	return value ? value : fdtdec_get_uint(fdt, chosen, name, 0);
}

static void comparison_reserve_ram_esp(void)
{
	const void *fdt = (const void *)get_prev_bl_fdt_addr();
	phys_addr_t address;
	u64 start, end;
	int chosen;

	if (!fdt || fdt_check_header(fdt))
		panic("COMPARISON: missing valid ABL FDT\n");
	chosen = fdt_path_offset(fdt, "/chosen");
	if (chosen < 0)
		panic("COMPARISON: missing ABL chosen node\n");
	start = comparison_initrd_property(fdt, chosen, "linux,initrd-start");
	end = comparison_initrd_property(fdt, chosen, "linux,initrd-end");
	if (!start || (start & (SZ_4K - 1)) || end <= start ||
	    end - start != SZ_64M)
		panic("COMPARISON: ABL RAM ESP is not aligned and 64 MiB\n");

	/* Reserve before board_late_init allocates any payload/fastboot buffers.
	 * LMB rejects overlap with U-Boot and DT reserved-memory, and propagates
	 * the reservation to EFI. blkmap's preserve flag alone does not do this.
	 */
	address = start;
	if (lmb_alloc_mem(LMB_MEM_ALLOC_ADDR, 0, &address, SZ_64M,
			  LMB_NOOVERWRITE))
		panic("COMPARISON: cannot reserve ABL RAM ESP\n");
	printf("COMPARISON RAM ESP RESERVED: %llx-%llx\n",
	       (unsigned long long)start, (unsigned long long)end);
}

