/*
 * Copyright 2026, Personal OS contributors
 * SPDX-License-Identifier: BSD-2-Clause
 *
 * Minimal VirtIO input event-queue spike for QEMU's keyboard and tablet.
 * This component owns the MMIO queues and publishes bounded event records to
 * an isolated Rust service. It never passes device mappings to applications.
 */
#include <microkit.h>
#include <stdint.h>
#include <sddf/util/printf.h>
#include <sddf/virtio/queue.h>
#include <sddf/virtio/transport/common.h>
#include <sddf/virtio/transport/mmio.h>
#include "input_handoff.h"

#define INPUT_DEVICE_ID 18
/* UTM tablet floods ABS samples; 8 descriptors refilled mid-IRQ used to mark
 * the device FAILED and freeze the guest cursor. Keep more in flight. */
#define INPUT_QUEUE_SIZE 32
#define INPUT_DEVICE_COUNT 2
#define INPUT_QUEUE_STRIDE 0x1000
/* Event buffers live above the virtqueue metadata (desc/avail/used). */
#define INPUT_EVENTS_OFFSET 0x800
#define VIRTIO_MMIO_KEYBOARD_OFFSET 0xc00
#define VIRTIO_MMIO_TABLET_OFFSET 0xa00
#define VIRTIO_F_VERSION_1 32
#define INPUT_EV_SYN 0U
#define INPUT_EV_KEY 1U
#define INPUT_EV_ABS 3U
#define INPUT_ABS_X 0U
#define INPUT_ABS_Y 1U

struct virtio_input_event {
    uint16_t type;
    uint16_t code;
    uint32_t value;
} __attribute__((packed));

uintptr_t virtio_regs;
uintptr_t input_queues;
uintptr_t input_queues_paddr;
uintptr_t input_handoff;

struct input_queue {
    volatile virtio_mmio_regs_t *regs;
    volatile struct virtq_desc *desc;
    volatile struct virtq_avail *avail;
    volatile struct virtq_used *used;
    volatile struct virtio_input_event *events;
    uint16_t next_used;
    const char *name;
};

static struct input_queue devices[INPUT_DEVICE_COUNT];
static struct input_event_ring *event_ring;

static volatile struct virtq_avail *avail_at(uintptr_t base)
{
    uintptr_t offset = INPUT_QUEUE_SIZE * sizeof(struct virtq_desc);
    offset = (offset + 1U) & ~1U;
    return (volatile struct virtq_avail *)(base + offset);
}

static volatile struct virtq_used *used_at(uintptr_t base)
{
    uintptr_t avail_offset = INPUT_QUEUE_SIZE * sizeof(struct virtq_desc);
    avail_offset = (avail_offset + 1U) & ~1U;
    uintptr_t used_offset = avail_offset + sizeof(uint16_t) * (2U + INPUT_QUEUE_SIZE);
    used_offset = (used_offset + 3U) & ~3U;
    return (volatile struct virtq_used *)(base + used_offset);
}

static void init_device(unsigned index, uintptr_t mmio_offset, const char *name)
{
    uintptr_t queue_base = input_queues + index * INPUT_QUEUE_STRIDE;
    uintptr_t queue_paddr = input_queues_paddr + index * INPUT_QUEUE_STRIDE;
    struct input_queue *device = &devices[index];
    device->regs = (volatile virtio_mmio_regs_t *)(virtio_regs + mmio_offset);
    device->desc = (volatile struct virtq_desc *)queue_base;
    device->avail = avail_at(queue_base);
    device->used = used_at(queue_base);
    device->events = (volatile struct virtio_input_event *)(queue_base + INPUT_EVENTS_OFFSET);
    device->name = name;
    device->next_used = 0;

    if (device->regs->MagicValue != VIRTIO_MMIO_MAGIC_VALUE ||
        device->regs->Version != VIRTIO_VERSION ||
        device->regs->DeviceID != INPUT_DEVICE_ID) {
        sddf_dprintf("INPUT|ERROR: %s VirtIO MMIO identity mismatch\n", name);
        return;
    }

    device->regs->Status = 0;
    device->regs->Status = VIRTIO_DEVICE_STATUS_ACKNOWLEDGE;
    device->regs->Status |= VIRTIO_DEVICE_STATUS_DRIVER;

    device->regs->DeviceFeaturesSel = 1;
    uint32_t features_high = device->regs->DeviceFeatures;
    if (!(features_high & (1U << (VIRTIO_F_VERSION_1 - 32)))) {
        sddf_dprintf("INPUT|ERROR: %s lacks VirtIO 1.x support\n", name);
        device->regs->Status |= VIRTIO_DEVICE_STATUS_FAILED;
        return;
    }
    device->regs->DriverFeaturesSel = 0;
    device->regs->DriverFeatures = 0;
    device->regs->DriverFeaturesSel = 1;
    device->regs->DriverFeatures = 1U << (VIRTIO_F_VERSION_1 - 32);
    device->regs->Status |= VIRTIO_DEVICE_STATUS_FEATURES_OK;
    if (!(device->regs->Status & VIRTIO_DEVICE_STATUS_FEATURES_OK)) {
        sddf_dprintf("INPUT|ERROR: %s rejected negotiated features\n", name);
        device->regs->Status |= VIRTIO_DEVICE_STATUS_FAILED;
        return;
    }

    for (uint16_t slot = 0; slot < INPUT_QUEUE_SIZE; slot++) {
        device->desc[slot].addr = queue_paddr + INPUT_EVENTS_OFFSET +
                                  slot * sizeof(struct virtio_input_event);
        device->desc[slot].len = sizeof(struct virtio_input_event);
        device->desc[slot].flags = VIRTQ_DESC_F_WRITE;
        device->desc[slot].next = 0;
        device->avail->ring[slot] = slot;
    }
    device->avail->flags = 0;
    device->avail->idx = INPUT_QUEUE_SIZE;
    device->used->flags = 0;
    device->used->idx = 0;
    __sync_synchronize();

    device->regs->QueueSel = 0; /* eventq */
    uint32_t maximum = device->regs->QueueNumMax;
    if (maximum < INPUT_QUEUE_SIZE) {
        sddf_dprintf("INPUT|ERROR: %s event queue too small (%u)\n", name, maximum);
        device->regs->Status |= VIRTIO_DEVICE_STATUS_FAILED;
        return;
    }
    device->regs->QueueNum = INPUT_QUEUE_SIZE;
    device->regs->QueueDescLow = (queue_paddr & 0xffffffffU);
    device->regs->QueueDescHigh = (queue_paddr >> 32);

    uintptr_t avail_paddr = queue_paddr +
        ((INPUT_QUEUE_SIZE * sizeof(struct virtq_desc) + 1U) & ~1U);
    uintptr_t used_paddr = queue_paddr +
        ((uintptr_t)((uint8_t *)device->used - (uint8_t *)queue_base));
    device->regs->QueueDriverLow = (avail_paddr & 0xffffffffU);
    device->regs->QueueDriverHigh = (avail_paddr >> 32);
    device->regs->QueueDeviceLow = (used_paddr & 0xffffffffU);
    device->regs->QueueDeviceHigh = (used_paddr >> 32);
    device->regs->QueueReady = 1;
    device->regs->Status |= VIRTIO_DEVICE_STATUS_DRIVER_OK;
    __sync_synchronize();
    device->regs->QueueNotify = 0;
    sddf_dprintf("INPUT|READY: %s VirtIO event queue active\n", name);
}

static void handle_events(unsigned index)
{
    struct input_queue *device = &devices[index];
    uint32_t interrupt_status = device->regs->InterruptStatus;
    if (interrupt_status & VIRTIO_IRQ_VQUEUE) {
        int published = 0;
        int have_abs_x = 0;
        int have_abs_y = 0;
        struct input_event_record abs_x = {0};
        struct input_event_record abs_y = {0};
        /* Drain everything currently used. Refills can race mid-handler; never
         * mark the device FAILED just because more samples arrived. Tablet ABS
         * floods the shared ring and starves keyboard/BTN clicks — coalesce to
         * the latest X/Y per IRQ and skip SYN (router only needs the point). */
        unsigned safety = 0;
        while (device->next_used != device->used->idx &&
               safety < INPUT_QUEUE_SIZE * 8U) {
            uint16_t used_index = device->next_used % INPUT_QUEUE_SIZE;
            uint32_t descriptor = device->used->ring[used_index].id;
            if (descriptor >= INPUT_QUEUE_SIZE) {
                sddf_dprintf("INPUT|ERROR: %s returned invalid descriptor %u\n",
                             device->name, descriptor);
                break;
            }
            volatile struct virtio_input_event *event = &device->events[descriptor];
            struct input_event_record record = {
                .device = index == 0 ? INPUT_DEVICE_KEYBOARD : INPUT_DEVICE_TABLET,
                .type = event->type,
                .code = event->code,
                .reserved = 0,
                .value = event->value,
            };
            if (record.device == INPUT_DEVICE_TABLET && record.type == INPUT_EV_SYN) {
                /* drop */
            } else if (record.device == INPUT_DEVICE_TABLET &&
                       record.type == INPUT_EV_ABS && record.code == INPUT_ABS_X) {
                abs_x = record;
                have_abs_x = 1;
            } else if (record.device == INPUT_DEVICE_TABLET &&
                       record.type == INPUT_EV_ABS && record.code == INPUT_ABS_Y) {
                abs_y = record;
                have_abs_y = 1;
            } else {
                /* Flush pending point before KEY/BTN so click coords match. */
                if (have_abs_x) {
                    published |= input_ring_try_push(event_ring, abs_x);
                    have_abs_x = 0;
                }
                if (have_abs_y) {
                    published |= input_ring_try_push(event_ring, abs_y);
                    have_abs_y = 0;
                }
                published |= input_ring_try_push(event_ring, record);
            }
            device->avail->ring[device->avail->idx % INPUT_QUEUE_SIZE] = descriptor;
            device->avail->idx++;
            device->next_used++;
            safety++;
        }
        if (have_abs_x) {
            published |= input_ring_try_push(event_ring, abs_x);
        }
        if (have_abs_y) {
            published |= input_ring_try_push(event_ring, abs_y);
        }
        if (published) {
            microkit_notify(INPUT_SERVICE_CHANNEL);
        }
        __sync_synchronize();
        device->regs->QueueNotify = 0;
    }
    if (interrupt_status != 0) {
        device->regs->InterruptACK = interrupt_status;
    }
}

void init(void)
{
    event_ring = (struct input_event_ring *)input_handoff;
    __atomic_store_n(&event_ring->head, 0, __ATOMIC_RELAXED);
    __atomic_store_n(&event_ring->tail, 0, __ATOMIC_RELAXED);
    __atomic_store_n(&event_ring->dropped, 0, __ATOMIC_RELAXED);
    init_device(0, VIRTIO_MMIO_KEYBOARD_OFFSET, "keyboard");
    init_device(1, VIRTIO_MMIO_TABLET_OFFSET, "tablet");
}

void notified(microkit_channel channel)
{
    if (channel < INPUT_DEVICE_COUNT) {
        handle_events(channel);
        microkit_deferred_irq_ack(channel);
    }
}
