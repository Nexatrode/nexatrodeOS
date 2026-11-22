#include "simplex_sqlite_init.h"

#include "vault_storage_microkit.h"

#include <limits.h>
#include <microkit.h>
#include <stdint.h>

enum {
  VAULT_CHANNEL = 0,
  CLOCK_CHANNEL = 2,
  ENTROPY_CHANNEL = 3,
  CLOCK_REQUEST_UNIX_TIME = 0x205,
  CLOCK_REPLY_OK = 0x100,
  ENTROPY_REQUEST_BYTES = 0x401,
  ENTROPY_REPLY_OK = 0x400,
  JULIAN_UNIX_EPOCH_MS = 210866760000000LL,
};

static NexVaultStorage storage;
static NexSqliteVfs vfs;
static int vfs_initialized;
uintptr_t blob_transfer;

static int simplex_randomness(void *context, int amount, char *buffer) {
  (void)context;
  if (amount < 0 || (amount != 0 && buffer == NULL)) return 0;
  int filled = 0;
  while (filled < amount) {
    microkit_msginfo reply = microkit_ppcall(
        ENTROPY_CHANNEL, microkit_msginfo_new(ENTROPY_REQUEST_BYTES, 0));
    if (microkit_msginfo_get_label(reply) != ENTROPY_REPLY_OK ||
        microkit_msginfo_get_count(reply) != 2)
      goto unavailable;
    for (unsigned word_index = 0; word_index < 2; ++word_index) {
      seL4_Word word = microkit_mr_get(word_index);
      for (unsigned byte_index = 0;
           byte_index < 8 && filled < amount;
           ++byte_index, ++filled)
        buffer[filled] = (char)(word >> (8 * byte_index));
    }
  }
  return amount;

unavailable:
  for (int i = 0; i < amount; ++i)
    ((volatile uint8_t *)buffer)[i] = 0;
  return 0;
}

static int simplex_current_time(void *context, sqlite3_int64 *julian_ms) {
  microkit_msginfo reply;
  uint64_t seconds;
  (void)context;
  if (julian_ms == NULL) return SQLITE_MISUSE;
  reply = microkit_ppcall(
      CLOCK_CHANNEL,
      microkit_msginfo_new(CLOCK_REQUEST_UNIX_TIME, 0));
  if (microkit_msginfo_get_label(reply) != CLOCK_REPLY_OK ||
      microkit_msginfo_get_count(reply) != 1)
    return SQLITE_IOERR;
  seconds = (uint64_t)microkit_mr_get(0);
  if (seconds > ((uint64_t)INT64_MAX - JULIAN_UNIX_EPOCH_MS) / 1000)
    return SQLITE_IOERR;
  *julian_ms = JULIAN_UNIX_EPOCH_MS + (sqlite3_int64)(seconds * 1000);
  return SQLITE_OK;
}

int nex_simplex_sqlite_vfs_init(volatile NexVaultTransferPage *transfer) {
  if (vfs_initialized) return SQLITE_OK;
  int rc = nex_vault_storage_init(
      &storage, VAULT_CHANNEL, transfer, nex_microkit_vault_call, NULL,
      simplex_randomness, simplex_current_time, NULL);
  if (rc != SQLITE_OK) return rc;
  rc = nex_sqlite_vfs_register(&vfs, nex_vault_storage_ops(), &storage,
                               "nexatrode-storage");
  if (rc == SQLITE_OK) vfs_initialized = 1;
  return rc;
}

int nex_simplex_sqlite_vfs_init_from_symbol(void) {
  if (blob_transfer == 0) return SQLITE_CANTOPEN;
  return nex_simplex_sqlite_vfs_init(
      (volatile NexVaultTransferPage *)blob_transfer);
}
