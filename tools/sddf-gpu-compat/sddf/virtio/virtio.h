#pragma once

#include <sddf/virtio/feature.h>
#include <sddf/virtio/transport/common.h>
#include <sddf/virtio/transport/mmio.h>

#define VIRTIO_MMIO_IRQ_VQUEUE VIRTIO_IRQ_VQUEUE
#define VIRTIO_MMIO_IRQ_CONFIG VIRTIO_IRQ_CONFIG

static inline bool virtio_mmio_check_magic(volatile virtio_mmio_regs_t *regs)
{
    return regs->MagicValue == VIRTIO_MMIO_MAGIC_VALUE;
}

static inline uint32_t virtio_mmio_version(volatile virtio_mmio_regs_t *regs)
{
    return regs->Version;
}

static inline bool virtio_mmio_check_device_id(volatile virtio_mmio_regs_t *regs,
                                                virtio_device_id_t device_id)
{
    return regs->DeviceID == (uint32_t)device_id;
}
