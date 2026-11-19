#include "vault_storage.h"

#include <limits.h>
#include <string.h>

enum {
  REPLY_OK = 0x300,
  REQUEST_FILE_READ = 0x430,
  REQUEST_FILE_WRITE = 0x431,
  REQUEST_FILE_TRUNCATE = 0x432,
  REQUEST_FILE_SYNC = 0x433,
  REQUEST_FILE_SIZE = 0x434,
  REQUEST_FILE_DELETE = 0x435,
};

static int valid_file(int file_id) {
  return file_id == NEX_FILE_DB || file_id == NEX_FILE_JOURNAL;
}

static void copy_from_transfer(unsigned char *destination,
                               const volatile unsigned char *source,
                               size_t length) {
  for (size_t i = 0; i < length; ++i)
    destination[i] = source[i];
}

static void copy_to_transfer(volatile unsigned char *destination,
                             const unsigned char *source, size_t length) {
  for (size_t i = 0; i < length; ++i)
    destination[i] = source[i];
}

static int call_vault(NexVaultStorage *storage, uint32_t label,
                      uintptr_t *mrs, unsigned input_count,
                      unsigned expected_output_count) {
  unsigned output_count = 0;
  uint32_t reply_label = 0;
  if (storage == NULL || storage->call == NULL ||
      storage->call(storage->call_context, storage->channel, label, mrs,
                    input_count, &output_count, &reply_label) != 0)
    return SQLITE_IOERR;
  if (reply_label != REPLY_OK || output_count != expected_output_count) {
    if (reply_label == 0x303)
      return SQLITE_FULL;
    return reply_label == 0x304 ? SQLITE_READONLY : SQLITE_IOERR;
  }
  return SQLITE_OK;
}

static int vault_read(void *context, int file_id, void *buffer, int amount,
                      sqlite3_int64 offset) {
  NexVaultStorage *storage = context;
  unsigned char *out = buffer;
  int copied = 0;
  if (!valid_file(file_id) || storage == NULL || storage->transfer == NULL ||
      buffer == NULL || amount < 0 || offset < 0 || offset > INT64_MAX - amount)
    return SQLITE_IOERR_READ;
  while (copied < amount) {
    int chunk = amount - copied;
    uintptr_t mrs[3];
    int rc;
    if (chunk > NEX_VAULT_PAYLOAD_CAPACITY)
      chunk = NEX_VAULT_PAYLOAD_CAPACITY;
    storage->transfer->length = 0;
    mrs[0] = (uintptr_t)file_id;
    mrs[1] = (uintptr_t)(offset + copied);
    mrs[2] = (uintptr_t)chunk;
    rc = call_vault(storage, REQUEST_FILE_READ, mrs, 3, 1);
    if (rc != SQLITE_OK || mrs[0] > (uintptr_t)chunk ||
        storage->transfer->length != (uint32_t)mrs[0])
      return SQLITE_IOERR_READ;
    if (mrs[0] != 0)
      copy_from_transfer(out + copied, storage->transfer->payload,
                         (size_t)mrs[0]);
    copied += (int)mrs[0];
    if (mrs[0] < (uintptr_t)chunk) {
      memset(out + copied, 0, (size_t)(amount - copied));
      return SQLITE_IOERR_SHORT_READ;
    }
  }
  return SQLITE_OK;
}

static int vault_write(void *context, int file_id, const void *buffer,
                       int amount, sqlite3_int64 offset) {
  NexVaultStorage *storage = context;
  const unsigned char *in = buffer;
  int copied = 0;
  if (!valid_file(file_id) || storage == NULL || storage->transfer == NULL ||
      buffer == NULL || amount < 0 || offset < 0 || offset > INT64_MAX - amount)
    return SQLITE_IOERR_WRITE;
  while (copied < amount) {
    int chunk = amount - copied;
    uintptr_t mrs[3];
    int rc;
    if (chunk > NEX_VAULT_PAYLOAD_CAPACITY)
      chunk = NEX_VAULT_PAYLOAD_CAPACITY;
    copy_to_transfer(storage->transfer->payload, in + copied, (size_t)chunk);
    storage->transfer->length = (uint32_t)chunk;
    mrs[0] = (uintptr_t)file_id;
    mrs[1] = (uintptr_t)(offset + copied);
    mrs[2] = (uintptr_t)chunk;
    rc = call_vault(storage, REQUEST_FILE_WRITE, mrs, 3, 0);
    if (rc != SQLITE_OK)
      return rc == SQLITE_FULL ? rc : SQLITE_IOERR_WRITE;
    copied += chunk;
  }
  return SQLITE_OK;
}

static int vault_truncate(void *context, int file_id, sqlite3_int64 size) {
  uintptr_t mrs[2];
  if (!valid_file(file_id) || size < 0)
    return SQLITE_IOERR_TRUNCATE;
  mrs[0] = (uintptr_t)file_id;
  mrs[1] = (uintptr_t)size;
  {
    int rc = call_vault(context, REQUEST_FILE_TRUNCATE, mrs, 2, 0);
    return rc == SQLITE_FULL ? rc : (rc == SQLITE_OK ? SQLITE_OK : SQLITE_IOERR_TRUNCATE);
  }
}

static int vault_sync(void *context, int file_id) {
  uintptr_t mrs[1];
  if (!valid_file(file_id))
    return SQLITE_IOERR_FSYNC;
  mrs[0] = (uintptr_t)file_id;
  return call_vault(context, REQUEST_FILE_SYNC, mrs, 1, 0) == SQLITE_OK
             ? SQLITE_OK
             : SQLITE_IOERR_FSYNC;
}

static int vault_size(void *context, int file_id, sqlite3_int64 *size) {
  uintptr_t mrs[2];
  if (!valid_file(file_id) || size == NULL)
    return SQLITE_IOERR_FSTAT;
  mrs[0] = (uintptr_t)file_id;
  if (call_vault(context, REQUEST_FILE_SIZE, mrs, 1, 2) != SQLITE_OK)
    return SQLITE_IOERR_FSTAT;
  if (mrs[0] > INT64_MAX)
    return SQLITE_IOERR_FSTAT;
  *size = (sqlite3_int64)mrs[0];
  return SQLITE_OK;
}

static int vault_delete(void *context, int file_id) {
  uintptr_t mrs[1];
  if (file_id != NEX_FILE_JOURNAL)
    return SQLITE_READONLY;
  mrs[0] = (uintptr_t)file_id;
  return call_vault(context, REQUEST_FILE_DELETE, mrs, 1, 0) == SQLITE_OK
             ? SQLITE_OK
             : SQLITE_IOERR_DELETE;
}

static int vault_exists(void *context, int file_id, int *exists) {
  uintptr_t mrs[2];
  if (!valid_file(file_id) || exists == NULL)
    return SQLITE_IOERR_ACCESS;
  mrs[0] = (uintptr_t)file_id;
  if (call_vault(context, REQUEST_FILE_SIZE, mrs, 1, 2) != SQLITE_OK)
    return SQLITE_IOERR_ACCESS;
  *exists = mrs[1] != 0;
  return SQLITE_OK;
}

static int vault_randomness(void *context, int amount, char *buffer) {
  NexVaultStorage *storage = context;
  if (storage == NULL || storage->randomness == NULL || amount < 0 ||
      (amount != 0 && buffer == NULL))
    return 0;
  return storage->randomness(storage->platform_context, amount, buffer);
}

static int vault_current_time(void *context, sqlite3_int64 *julian_ms) {
  NexVaultStorage *storage = context;
  if (storage == NULL || storage->current_time == NULL || julian_ms == NULL)
    return SQLITE_IOERR;
  return storage->current_time(storage->platform_context, julian_ms);
}

static const NexStorageOps ops = {
    .read = vault_read,
    .write = vault_write,
    .truncate = vault_truncate,
    .sync = vault_sync,
    .size = vault_size,
    .delete_file = vault_delete,
    .exists = vault_exists,
    .randomness = vault_randomness,
    .current_time = vault_current_time,
};

int nex_vault_storage_init(NexVaultStorage *storage, unsigned channel,
                           volatile NexVaultTransferPage *transfer,
                           NexVaultCall call, void *call_context,
                           NexVaultRandom randomness, NexVaultClock current_time,
                           void *platform_context) {
  if (storage == NULL || transfer == NULL || call == NULL ||
      randomness == NULL || current_time == NULL)
    return SQLITE_MISUSE;
  storage->channel = channel;
  storage->transfer = transfer;
  storage->call = call;
  storage->call_context = call_context;
  storage->randomness = randomness;
  storage->current_time = current_time;
  storage->platform_context = platform_context;
  return SQLITE_OK;
}

const NexStorageOps *nex_vault_storage_ops(void) { return &ops; }
