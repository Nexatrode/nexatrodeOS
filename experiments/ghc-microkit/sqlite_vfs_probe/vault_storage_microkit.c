#include "vault_storage_microkit.h"

#include <microkit.h>
#include <stddef.h>

/* Keep a short trace of the first AgentStore storage calls. This helps
 * distinguish VFS-open failures from a bad vault IPC route in the guest log. */
static unsigned trace_calls;

static void trace_hex(uint32_t value) {
  static const char digits[] = "0123456789ABCDEF";
  for (int shift = 28; shift >= 0; shift -= 4)
    microkit_dbg_putc(digits[(value >> shift) & 0xFu]);
}

int nex_microkit_vault_call(void *context, unsigned channel, uint32_t label,
                            uintptr_t *mrs, unsigned input_count,
                            unsigned *output_count, uint32_t *reply_label) {
  microkit_msginfo reply;
  (void)context;
  if (mrs == NULL || output_count == NULL || reply_label == NULL ||
      input_count > seL4_MsgMaxLength)
    return -1;
  unsigned trace_index = trace_calls++;
  if (trace_index < 20) {
    microkit_dbg_puts("SIMPLEX_VAULT|REQ|");
    trace_hex(label);
    microkit_dbg_puts("\n");
  }
  for (unsigned i = 0; i < input_count; ++i)
    microkit_mr_set((seL4_Uint8)i, (seL4_Word)mrs[i]);
  reply = microkit_ppcall(channel,
                          microkit_msginfo_new((seL4_Word)label,
                                               (seL4_Uint16)input_count));
  *reply_label = (uint32_t)microkit_msginfo_get_label(reply);
  *output_count = (unsigned)microkit_msginfo_get_count(reply);
  if (trace_index < 20) {
    microkit_dbg_puts("SIMPLEX_VAULT|REPLY|");
    trace_hex(*reply_label);
    microkit_dbg_putc('|');
    trace_hex(*output_count);
    microkit_dbg_puts("\n");
  }
  if (*output_count > seL4_MsgMaxLength)
    return -1;
  for (unsigned i = 0; i < *output_count; ++i)
    mrs[i] = (uintptr_t)microkit_mr_get((seL4_Uint8)i);
  return 0;
}
