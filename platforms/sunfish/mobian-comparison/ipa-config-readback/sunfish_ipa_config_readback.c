// SPDX-License-Identifier: GPL-2.0-only
/* One read-only snapshot of two IPA registers on the pinned Sunfish Linux.
 * Refuses to wake a runtime-suspended device or touch a different platform.
 */
#include <linux/io.h>
#include <linux/module.h>
#include <linux/of.h>
#include <linux/of_address.h>
#include <linux/of_platform.h>
#include <linux/platform_device.h>
#include <linux/pm_runtime.h>

#define EXPECTED_BASE 0x01e40000ULL
#define EXPECTED_SIZE 0x7000
#define CLKON_CFG 0x44
#define QSB_MAX_WRITES 0x74

static int __init sunfish_ipa_config_readback_init(void)
{
	struct device_node *node;
	struct platform_device *pdev;
	struct resource resource;
	void __iomem *base;
	u32 clkon_first, qsb_first, clkon_second, qsb_second;
	int ret;

	if (!of_machine_is_compatible("google,sunfish"))
		return -ENODEV;
	node = of_find_compatible_node(NULL, NULL, "qcom,sm7150-ipa");
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
		goto put_device;
	}
	device_lock(&pdev->dev);
	if (!pdev->dev.driver || !pm_runtime_enabled(&pdev->dev)) {
		ret = -ENODEV;
		goto unlock;
	}
	/* Pin only if already active; never change power or reset state. */
	ret = pm_runtime_get_if_active(&pdev->dev);
	if (ret <= 0) {
		ret = ret < 0 ? ret : -EAGAIN;
		goto unlock;
	}
	base = ioremap(resource.start, resource_size(&resource));
	if (!base) {
		ret = -ENOMEM;
		goto runtime_put;
	}
	clkon_first = readl(base + CLKON_CFG);
	qsb_first = readl(base + QSB_MAX_WRITES);
	clkon_second = readl(base + CLKON_CFG);
	qsb_second = readl(base + QSB_MAX_WRITES);
	pr_info("sunfish-ipa-config: clkon=%08x/%08x qsb-writes=%08x/%08x stable=%u\n",
		clkon_first, clkon_second, qsb_first, qsb_second,
		clkon_first == clkon_second && qsb_first == qsb_second);
	iounmap(base);
	ret = 0;
runtime_put:
	pm_runtime_put(&pdev->dev);
unlock:
	device_unlock(&pdev->dev);
put_device:
	put_device(&pdev->dev);
	return ret;
}

static void __exit sunfish_ipa_config_readback_exit(void) { }
module_init(sunfish_ipa_config_readback_init);
module_exit(sunfish_ipa_config_readback_exit);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("Active-only read-only Sunfish IPA clock and QSB snapshot");
