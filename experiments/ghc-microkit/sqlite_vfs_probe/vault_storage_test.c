#include "vault_storage.h"

#include <assert.h>
#include <stdio.h>
#include <string.h>

#define FILE_CAPACITY (4 * 1024 * 1024)

typedef struct {
  unsigned char bytes[NEX_FILE_COUNT][FILE_CAPACITY];
  sqlite3_int64 size[NEX_FILE_COUNT];
  int exists[NEX_FILE_COUNT];
  unsigned calls;
  unsigned write_calls;
  unsigned sync_calls;
  unsigned delete_calls;
  unsigned last_channel;
  unsigned char *page;
} FakeVault;

static int fake_call(void *context, unsigned channel, uint32_t label,
                     uintptr_t *mrs, unsigned input_count,
                     unsigned *output_count, uint32_t *reply_label) {
  FakeVault *vault = context;
  int id;
  ++vault->calls;
  vault->last_channel = channel;
  *output_count = 0;
  *reply_label = 0x300;
  if (input_count == 0 || mrs[0] >= NEX_FILE_COUNT) {
    *reply_label = 0x303;
    return 0;
  }
  id = (int)mrs[0];
  switch (label) {
  case 0x430: {
    sqlite3_int64 offset;
    size_t length;
    if (input_count != 3 || mrs[1] > FILE_CAPACITY || mrs[2] > NEX_VAULT_PAYLOAD_CAPACITY) {
      *reply_label = 0x303;
      return 0;
    }
    offset = (sqlite3_int64)mrs[1];
    length = (size_t)mrs[2];
    if (offset >= vault->size[id])
      length = 0;
    else if (length > (size_t)(vault->size[id] - offset))
      length = (size_t)(vault->size[id] - offset);
    memcpy(vault->page + sizeof(uint32_t), vault->bytes[id] + offset, length);
    *(uint32_t *)vault->page = (uint32_t)length;
    mrs[0] = length;
    *output_count = 1;
    return 0;
  }
  case 0x431: {
    sqlite3_int64 offset;
    size_t length;
    if (input_count != 3 || mrs[1] > FILE_CAPACITY || mrs[2] > NEX_VAULT_PAYLOAD_CAPACITY) {
      *reply_label = 0x303;
      return 0;
    }
    offset = (sqlite3_int64)mrs[1];
    length = (size_t)mrs[2];
    if (offset + (sqlite3_int64)length > FILE_CAPACITY) {
      *reply_label = 0x303;
      return 0;
    }
    memcpy(vault->bytes[id] + offset, vault->page + sizeof(uint32_t), length);
    if (offset + (sqlite3_int64)length > vault->size[id])
      vault->size[id] = offset + (sqlite3_int64)length;
    vault->exists[id] = 1;
    ++vault->write_calls;
    return 0;
  }
  case 0x432:
    if (input_count != 2 || mrs[1] > FILE_CAPACITY) {
      *reply_label = 0x303;
      return 0;
    }
    if (mrs[1] > (uintptr_t)vault->size[id])
      memset(vault->bytes[id] + vault->size[id], 0,
             (size_t)(mrs[1] - (uintptr_t)vault->size[id]));
    vault->size[id] = (sqlite3_int64)mrs[1];
    vault->exists[id] = 1;
    return 0;
  case 0x433:
    if (input_count != 1) {
      *reply_label = 0x303;
      return 0;
    }
    ++vault->sync_calls;
    return 0;
  case 0x434:
    if (input_count != 1) {
      *reply_label = 0x303;
      return 0;
    }
    mrs[0] = (uintptr_t)vault->size[id];
    mrs[1] = (uintptr_t)vault->exists[id];
    *output_count = 2;
    return 0;
  case 0x435:
    if (input_count != 1 || id != NEX_FILE_JOURNAL) {
      *reply_label = 0x304;
      return 0;
    }
    vault->size[id] = 0;
    vault->exists[id] = 0;
    ++vault->delete_calls;
    return 0;
  default:
    *reply_label = 0x303;
    return 0;
  }
}

static int fake_random(void *context, int amount, char *buffer) {
  (void)context;
  for (int i = 0; i < amount; ++i)
    buffer[i] = (char)(i * 31 + 7);
  return amount;
}

static int fake_time(void *context, sqlite3_int64 *julian_ms) {
  (void)context;
  *julian_ms = 210866760000000LL;
  return SQLITE_OK;
}

int main(void) {
  static FakeVault vault;
  static NexVaultStorage storage;
  static NexVaultTransferPage transfer;
  unsigned char source[9000];
  unsigned char target[9000];
  sqlite3_int64 size = -1;
  int exists = 0;
  for (size_t i = 0; i < sizeof(source); ++i)
    source[i] = (unsigned char)(i * 13u);
  vault.page = (unsigned char *)&transfer;
  assert(nex_vault_storage_init(&storage, 6, &transfer, fake_call, &vault,
                                fake_random, fake_time, NULL) == SQLITE_OK);
  {
    const NexStorageOps *ops = nex_vault_storage_ops();
    assert(ops->write(&storage, NEX_FILE_DB, source, sizeof(source), 0) == SQLITE_OK);
    assert(vault.write_calls == 3);
    assert(vault.last_channel == 6);
    assert(ops->sync(&storage, NEX_FILE_DB) == SQLITE_OK);
    assert(vault.sync_calls == 1);
    assert(ops->size(&storage, NEX_FILE_DB, &size) == SQLITE_OK);
    assert(size == (sqlite3_int64)sizeof(source));
    assert(ops->exists(&storage, NEX_FILE_DB, &exists) == SQLITE_OK && exists);
    memset(target, 0, sizeof(target));
    assert(ops->read(&storage, NEX_FILE_DB, target, sizeof(target), 0) == SQLITE_OK);
    assert(memcmp(source, target, sizeof(source)) == 0);
    assert(ops->truncate(&storage, NEX_FILE_DB, 5) == SQLITE_OK);
    assert(ops->size(&storage, NEX_FILE_DB, &size) == SQLITE_OK && size == 5);
    assert(ops->delete_file(&storage, NEX_FILE_DB) == SQLITE_READONLY);
    assert(ops->delete_file(&storage, NEX_FILE_JOURNAL) == SQLITE_OK);
    assert(vault.delete_calls == 1);
    memset(target, 0xff, sizeof(target));
    assert(ops->read(&storage, NEX_FILE_DB, target, 10, 0) == SQLITE_IOERR_SHORT_READ);
    assert(memcmp(target, vault.bytes[NEX_FILE_DB], 5) == 0);
    for (size_t i = 5; i < 10; ++i)
      assert(target[i] == 0);
    assert(ops->write(&storage, NEX_FILE_DB, source, 1, FILE_CAPACITY) == SQLITE_FULL);
    assert(ops->randomness(&storage, 4, (char *)target) == 4);
    assert((unsigned char)target[0] == 7 && (unsigned char)target[3] == 100);
    assert(ops->current_time(&storage, &size) == SQLITE_OK);
    assert(size == 210866760000000LL);
  }
  puts("VAULT_STORAGE|PASS|chunked IPC, bounds, short reads, sync and journal policy");
  return 0;
}
