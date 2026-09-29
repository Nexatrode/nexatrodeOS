#pragma once

#include <sddf/virtio/queue.h>
#include <sddf/virtio/transport/mmio.h>

static inline void virtio_queue_notify(volatile virtio_mmio_regs_t *regs,
                                       uint32_t queue_index)
{
    regs->QueueNotify = queue_index;
}
