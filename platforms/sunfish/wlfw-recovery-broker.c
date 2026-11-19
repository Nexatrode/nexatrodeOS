#include <stdint.h>
#include <microkit.h>

#define RECOVERY_SUBMIT 0x610u
#define RECOVERY_ACCEPT 0x611u
#define RECOVERY_REJECT 0x612u
#define RECOVERY_QUEUE_MAGIC 0x51465752u
#define RECOVERY_CONTROL_MAGIC 0x43565752u

struct recovery_control {
	uint32_t magic;
	uint32_t session_id;
	uint32_t restart_count;
	uint32_t result;
};

struct broker_queue {
	uint32_t magic;
	uint32_t session_id;
	uint32_t produced;
	uint32_t consumed;
	uint32_t marker;
};

uintptr_t control_vaddr;
uintptr_t queue_vaddr;
static uint32_t private_init_count;

static volatile struct recovery_control *control_page(void)
{
	return (volatile struct recovery_control *)control_vaddr;
}

static volatile struct broker_queue *queue_page(void)
{
	return (volatile struct broker_queue *)queue_vaddr;
}

static void trigger_test_fault(void)
{
	volatile uint32_t *bad = (volatile uint32_t *)0;
	*bad = 0xdeadbeefu;
}

void init(void)
{
	private_init_count++;
	volatile struct recovery_control *control = control_page();
	volatile struct broker_queue *queue = queue_page();
	if (control->magic != RECOVERY_CONTROL_MAGIC ||
	    queue->magic != RECOVERY_QUEUE_MAGIC) {
		microkit_dbg_puts("WLFW RECOVERY BROKER BAD CONTROL\r\n");
		return;
	}

	if (control->restart_count == 0u) {
		if (private_init_count != 1u || control->session_id != 1u ||
		    queue->session_id != 1u || queue->produced != 0u) {
			microkit_dbg_puts("WLFW RECOVERY FIRST START FAIL\r\n");
			return;
		}
		queue->session_id = control->session_id;
		queue->produced = 1u;
		queue->consumed = 0u;
		queue->marker = 0x5354414cu;
		__asm__ volatile("dsb sy" ::: "memory");
		microkit_dbg_puts("WLFW RECOVERY INJECTING BROKER FAULT\r\n");
		trigger_test_fault();
		return;
	}

	if (control->restart_count != 1u || control->session_id != 2u ||
	    private_init_count != 2u || queue->session_id != 2u ||
	    queue->produced != 0u || queue->consumed != 0u || queue->marker != 0u) {
		microkit_dbg_puts("WLFW RECOVERY RESTART STATE FAIL\r\n");
		return;
	}

	queue->produced = 1u;
	queue->marker = 0x46524553u;
	__asm__ volatile("dsb sy" ::: "memory");
	microkit_mr_set(0, control->session_id);
	microkit_mr_set(1, queue->marker);
	microkit_msginfo reply = microkit_ppcall(
		0, microkit_msginfo_new(RECOVERY_SUBMIT, 2));
	if (microkit_msginfo_get_label(reply) != RECOVERY_ACCEPT ||
	    microkit_msginfo_get_count(reply) != 0u) {
		microkit_dbg_puts("WLFW RECOVERY SUPERVISOR REJECTED NEW SESSION\r\n");
		return;
	}
	microkit_dbg_puts("WLFW RECOVERY RESTARTED BROKER ACCEPTED\r\n");
}

void notified(microkit_channel channel)
{
	(void)channel;
}
