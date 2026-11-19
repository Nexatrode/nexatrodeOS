#ifndef NEXATRODE_VAULT_STORAGE_MICROKIT_H
#define NEXATRODE_VAULT_STORAGE_MICROKIT_H

#include <stdint.h>

/* Adapter for a Messages-app PD whose vault capability is channel 0. */
int nex_microkit_vault_call(void *context, unsigned channel, uint32_t label,
                            uintptr_t *mrs, unsigned input_count,
                            unsigned *output_count, uint32_t *reply_label);

#endif
