#ifndef NEXATRODE_VAULT_STORAGE_H
#define NEXATRODE_VAULT_STORAGE_H

#include "vfs.h"

#include <stdint.h>

/* Must match Rust BlobTransferPage: u32 length followed by 4036 bytes. */
enum { NEX_VAULT_PAYLOAD_CAPACITY = 4036 };
typedef struct {
  uint32_t length;
  unsigned char payload[NEX_VAULT_PAYLOAD_CAPACITY];
} NexVaultTransferPage;

typedef int (*NexVaultCall)(void *context, unsigned channel, uint32_t label,
                            uintptr_t *mrs, unsigned input_count,
                            unsigned *output_count, uint32_t *reply_label);
typedef int (*NexVaultRandom)(void *context, int amount, char *buffer);
typedef int (*NexVaultClock)(void *context, sqlite3_int64 *julian_ms);

typedef struct {
  unsigned channel;
  volatile NexVaultTransferPage *transfer;
  NexVaultCall call;
  void *call_context;
  NexVaultRandom randomness;
  NexVaultClock current_time;
  void *platform_context;
} NexVaultStorage;

/* Register this storage backend with nex_sqlite_vfs_register(). */
int nex_vault_storage_init(NexVaultStorage *storage, unsigned channel,
                           volatile NexVaultTransferPage *transfer,
                           NexVaultCall call, void *call_context,
                           NexVaultRandom randomness, NexVaultClock current_time,
                           void *platform_context);
const NexStorageOps *nex_vault_storage_ops(void);

#endif
