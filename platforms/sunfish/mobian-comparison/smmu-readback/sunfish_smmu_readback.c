// SPDX-License-Identifier: GPL-2.0-only
/* Read-only snapshot of the running, pinned Linux Sunfish Apps SMMU.
 * No register writes, translation requests, driver unbind or power-on calls.
 */
#include <linux/io.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/of_address.h>
#include <linux/of_platform.h>
#include <linux/platform_device.h>
#include <linux/pm_runtime.h>
#include <linux/slab.h>

#define EXPECTED_BASE 0x15000000ULL
#define EXPECTED_SIZE 0x100000
#define MAX_GROUPS 128
#define MAX_BANKS 61
#define ID0 0x20
#define ID1 0x24
#define GLOBAL_FSR 0x48
#define SMR(i) (0x800 + 4 * (i))
#define S2CR(i) (0xc00 + 4 * (i))

struct route {
	u32 smr;
	u32 s2cr;
};

static int __init sunfish_smmu_readback_init(void)
{
	struct device_node *node;
	struct platform_device *pdev;
	struct resource resource;
	struct route *before;
	void __iomem *base;
	u32 id0, id1, groups, banks, page, cb_base;
	u32 scr0, fsr;
	bool pinned = false;
	bool changed = false;
	int ret = 0;
	unsigned int i;

	if (!of_machine_is_compatible("google,sunfish"))
		return -ENODEV;
	node = of_find_compatible_node(NULL, NULL, "qcom,sm7150-smmu-500");
	if (!node)
		return -ENODEV;
	pdev = of_find_device_by_node(node);
	ret = of_address_to_resource(node, 0, &resource);
	of_node_put(node);
	if (!pdev)
		return -ENODEV;
	if (ret || resource.start != EXPECTED_BASE ||
	    resource_size(&resource) != EXPECTED_SIZE) {
		ret = -EINVAL;
		goto put;
	}
	device_lock(&pdev->dev);
	if (!pdev->dev.driver) {
		ret = -ENODEV;
		goto unlock;
	}
	/* Hold an already-active device; never wake or reconfigure it. */
	if (pm_runtime_enabled(&pdev->dev)) {
        ret = pm_runtime_get_if_active(&pdev->dev);
		if (ret <= 0) {
			ret = ret < 0 ? ret : -EAGAIN;
			goto unlock;
		}
		pinned = true;
	}
	base = ioremap(resource.start, resource_size(&resource));
	if (!base) {
		ret = -ENOMEM;
		goto runtime;
	}
	id0 = readl(base + ID0);
	id1 = readl(base + ID1);
	groups = id0 & 0xff;
	banks = id1 & 0xff;
	page = id1 & BIT(31) ? 0x10000 : 0x1000;
	cb_base = page * (1U << (((id1 >> 28) & 7) + 1));
	if (!(id0 & BIT(27)) || !groups || groups > MAX_GROUPS ||
	    !banks || banks > MAX_BANKS || cb_base + banks * page > EXPECTED_SIZE) {
		ret = -EINVAL;
		goto unmap;
	}
	before = kcalloc(groups, sizeof(*before), GFP_KERNEL);
	if (!before) {
		ret = -ENOMEM;
		goto unmap;
	}
	scr0 = readl(base);
	fsr = readl(base + GLOBAL_FSR);
	for (i = 0; i < groups; ++i) {
		before[i].smr = readl(base + SMR(i));
		before[i].s2cr = readl(base + S2CR(i));
	}
	pr_info("SUNFISH_LINUX_SMMU id0=%08x id1=%08x groups=%u banks=%u page=%x cb_base=%x scr0=%08x gfsr=%08x\n",
		id0, id1, groups, banks, page, cb_base, scr0, fsr);
	for (i = 0; i < groups; ++i)
		pr_info("SUNFISH_LINUX_SMMU_ROUTE slot=%u smr=%08x s2cr=%08x\n",
			i, before[i].smr, before[i].s2cr);
	for (i = 0; i < banks; ++i) {
		void __iomem *cb = base + cb_base + i * page;
		u32 cbar = readl(base + page + 4 * i);
		u32 cba2r = readl(base + page + 0x800 + 4 * i);
		pr_info("SUNFISH_LINUX_SMMU_CB bank=%u cbar=%08x cba2r=%08x sctlr=%08x actlr=%08x ttbcr2=%08x ttbr0_lo=%08x ttbr0_hi=%08x ttbr1_lo=%08x ttbr1_hi=%08x ttbcr=%08x mair0=%08x mair1=%08x fsr=%08x\n",
			i, cbar, cba2r, readl(cb), readl(cb + 4), readl(cb + 0x10),
			readl(cb + 0x20), readl(cb + 0x24), readl(cb + 0x28),
			readl(cb + 0x2c), readl(cb + 0x30), readl(cb + 0x38),
			readl(cb + 0x3c), readl(cb + 0x58));
	}
	for (i = 0; i < groups; ++i)
		if (before[i].smr != readl(base + SMR(i)) ||
		    before[i].s2cr != readl(base + S2CR(i)))
			changed = true;
	if (id0 != readl(base + ID0) || id1 != readl(base + ID1) ||
	    scr0 != readl(base) || fsr != readl(base + GLOBAL_FSR))
		changed = true;
	pr_info("SUNFISH_LINUX_SMMU_DONE route_or_global_changed=%u register_writes=0\n", changed);
	kfree(before);
unmap:
	iounmap(base);
runtime:
	if (pinned)
		pm_runtime_put_noidle(&pdev->dev);
unlock:
	device_unlock(&pdev->dev);
put:
	put_device(&pdev->dev);
	return ret;
}

static void __exit sunfish_smmu_readback_exit(void) {}
module_init(sunfish_smmu_readback_init);
module_exit(sunfish_smmu_readback_exit);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("Read-only Sunfish Linux reference SMMU snapshot");
