/* QEMU-only SQLCipher execution test. Entropy comes from the QEMU virtio RNG
 * service; storage remains an in-memory fixture, not durable vault storage. */
#include "vfs.h"

#include <microkit.h>
#include <string.h>

#define FILE_CAPACITY (128u * 1024u)

enum {
  ENTROPY_CHANNEL = 0,
  ENTROPY_REQUEST_BYTES = 0x401,
  ENTROPY_REPLY_OK = 0x400,
};

typedef struct {
  unsigned char bytes[NEX_FILE_COUNT][FILE_CAPACITY];
  sqlite3_int64 size[NEX_FILE_COUNT];
  int exists[NEX_FILE_COUNT];
  int randomness_calls;
} TestStore;

static TestStore store;
static NexSqliteVfs vfs_state;
static int entropy_ipc_failed;

static int store_read(void *ctx, int id, void *buffer, int amount,
                      sqlite3_int64 offset) {
  TestStore *s = ctx;
  if (offset < 0 || amount < 0 || id < 0 || id >= NEX_FILE_COUNT)
    return SQLITE_IOERR_READ;
  if (offset >= s->size[id]) {
    memset(buffer, 0, (size_t)amount);
    return SQLITE_IOERR_SHORT_READ;
  }
  sqlite3_int64 available = s->size[id] - offset;
  if (available < amount) {
    memcpy(buffer, s->bytes[id] + offset, (size_t)available);
    memset((unsigned char *)buffer + available, 0,
           (size_t)(amount - available));
    return SQLITE_IOERR_SHORT_READ;
  }
  memcpy(buffer, s->bytes[id] + offset, (size_t)amount);
  return SQLITE_OK;
}

static int store_write(void *ctx, int id, const void *buffer, int amount,
                       sqlite3_int64 offset) {
  TestStore *s = ctx;
  if (offset < 0 || amount < 0 || id < 0 || id >= NEX_FILE_COUNT ||
      (uint64_t)offset + (uint64_t)amount > FILE_CAPACITY)
    return SQLITE_FULL;
  memcpy(s->bytes[id] + offset, buffer, (size_t)amount);
  if (offset + amount > s->size[id]) s->size[id] = offset + amount;
  s->exists[id] = 1;
  return SQLITE_OK;
}

static int store_truncate(void *ctx, int id, sqlite3_int64 size) {
  TestStore *s = ctx;
  if (id < 0 || id >= NEX_FILE_COUNT || size < 0 ||
      (uint64_t)size > FILE_CAPACITY)
    return SQLITE_FULL;
  if (size > s->size[id])
    memset(s->bytes[id] + s->size[id], 0, (size_t)(size - s->size[id]));
  if (size < s->size[id])
    memset(s->bytes[id] + size, 0, (size_t)(s->size[id] - size));
  s->size[id] = size;
  s->exists[id] = 1;
  return SQLITE_OK;
}

static int store_sync(void *ctx, int id) {
  (void)ctx;
  (void)id;
  return SQLITE_OK;
}

static int store_size(void *ctx, int id, sqlite3_int64 *size) {
  TestStore *s = ctx;
  if (id < 0 || id >= NEX_FILE_COUNT || size == NULL) return SQLITE_IOERR_FSTAT;
  *size = s->size[id];
  return SQLITE_OK;
}

static int store_delete(void *ctx, int id) {
  TestStore *s = ctx;
  if (id < 0 || id >= NEX_FILE_COUNT) return SQLITE_IOERR_DELETE;
  memset(s->bytes[id], 0, sizeof(s->bytes[id]));
  s->size[id] = 0;
  s->exists[id] = 0;
  return SQLITE_OK;
}

static int store_exists(void *ctx, int id, int *exists) {
  TestStore *s = ctx;
  if (id < 0 || id >= NEX_FILE_COUNT || exists == NULL)
    return SQLITE_IOERR_ACCESS;
  *exists = s->exists[id];
  return SQLITE_OK;
}

static int store_randomness(void *ctx, int amount, char *buffer) {
  TestStore *s = ctx;
  int filled = 0;
  if (amount < 0 || (amount > 0 && buffer == NULL)) return 0;
  ++s->randomness_calls;
  while (filled < amount) {
    microkit_msginfo reply = microkit_ppcall(
        ENTROPY_CHANNEL, microkit_msginfo_new(ENTROPY_REQUEST_BYTES, 0));
    if (microkit_msginfo_get_label(reply) != ENTROPY_REPLY_OK ||
        microkit_msginfo_get_count(reply) != 2) {
      entropy_ipc_failed = 1;
      memset(buffer, 0, (size_t)amount);
      return 0;
    }
    for (unsigned word_index = 0; word_index < 2; ++word_index) {
      seL4_Word word = microkit_mr_get(word_index);
      for (unsigned byte_index = 0; byte_index < 8 && filled < amount;
           ++byte_index, ++filled)
        buffer[filled] = (char)(word >> (8 * byte_index));
    }
  }
  return amount;
}

static int store_time(void *ctx, sqlite3_int64 *julian_ms) {
  (void)ctx;
  if (julian_ms == NULL) return SQLITE_IOERR;
  *julian_ms = 210866760000000LL + 1760000000000LL;
  return SQLITE_OK;
}

static const NexStorageOps store_ops = {
    .read = store_read,
    .write = store_write,
    .truncate = store_truncate,
    .sync = store_sync,
    .size = store_size,
    .delete_file = store_delete,
    .exists = store_exists,
    .randomness = store_randomness,
    .current_time = store_time,
};

static void fail(const char *where, int rc) {
  (void)rc;
  if (entropy_ipc_failed)
    microkit_dbg_puts("SQLCIPHER_QEMU|FAIL_ENTROPY_IPC\n");
  else if (where[0] == 'r')
    microkit_dbg_puts("SQLCIPHER_QEMU|FAIL_REGISTER\n");
  else if (where[0] == 'e')
    microkit_dbg_puts("SQLCIPHER_QEMU|FAIL_ENTROPY_NOT_REQUESTED\n");
  else if (where[0] == 'o')
    microkit_dbg_puts("SQLCIPHER_QEMU|FAIL_OPEN\n");
  else if (where[0] == 'k')
    microkit_dbg_puts("SQLCIPHER_QEMU|FAIL_KEY\n");
  else if (where[0] == 'c')
    microkit_dbg_puts("SQLCIPHER_QEMU|FAIL_CREATE\n");
  else
    microkit_dbg_puts("SQLCIPHER_QEMU|FAIL_READBACK\n");
  for (;;) {}
}

static sqlite3 *open_keyed(const char *key) {
  sqlite3 *db = NULL;
  int rc = sqlite3_open_v2("agent.db", &db,
                           SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                           "nexatrode-storage");
  if (rc != SQLITE_OK) fail("open", rc);
  rc = sqlite3_key(db, key, (int)strlen(key));
  if (rc != SQLITE_OK) fail("key", rc);
  return db;
}

void init(void) {
  sqlite3 *db;
  sqlite3_stmt *stmt = NULL;
  int rc;

  rc = nex_sqlite_vfs_register(&vfs_state, &store_ops, &store,
                               "nexatrode-storage");
  if (rc != SQLITE_OK) fail("register", rc);

  db = open_keyed("qemu-only-test-key");
  if (store.randomness_calls == 0) fail("entropy-not-requested", 1);
  microkit_dbg_puts("SQLCIPHER_QEMU|ENTROPY_IPC_OK\n");
  rc = sqlite3_exec(db,
                    "PRAGMA journal_mode=DELETE;"
                    "CREATE TABLE messages(body TEXT NOT NULL);"
                    "INSERT INTO messages VALUES('encrypted target readback');",
                    NULL, NULL, NULL);
  if (rc != SQLITE_OK) fail("create-insert", rc);
  rc = sqlite3_close(db);
  if (rc != SQLITE_OK) fail("close-created", rc);
  if (store.size[NEX_FILE_DB] < 16 ||
      memcmp(store.bytes[NEX_FILE_DB], "SQLite format 3", 15) == 0)
    fail("plaintext-header", 1);
  microkit_dbg_puts("SQLCIPHER_QEMU|CIPHERTEXT_HEADER_OK\n");

  db = open_keyed("wrong-qemu-test-key");
  rc = sqlite3_prepare_v2(db, "SELECT body FROM messages", -1, &stmt, NULL);
  if (rc == SQLITE_OK) {
    rc = sqlite3_step(stmt);
    if (rc == SQLITE_ROW) fail("wrong-key-returned-row", rc);
    sqlite3_finalize(stmt);
  }
  sqlite3_close(db);
  microkit_dbg_puts("SQLCIPHER_QEMU|WRONG_KEY_REJECTED\n");

  db = open_keyed("qemu-only-test-key");
  rc = sqlite3_prepare_v2(db, "SELECT body FROM messages", -1, &stmt, NULL);
  if (rc != SQLITE_OK) fail("prepare-readback", rc);
  rc = sqlite3_step(stmt);
  if (rc != SQLITE_ROW ||
      strcmp((const char *)sqlite3_column_text(stmt, 0),
             "encrypted target readback") != 0)
    fail("readback", rc);
  sqlite3_finalize(stmt);
  sqlite3_close(db);
  microkit_dbg_puts("SQLCIPHER_QEMU|PASS|encrypted header, wrong-key rejection, reopen\n");
}

void notified(microkit_channel channel) { (void)channel; }

microkit_msginfo protected(microkit_channel channel,
                           microkit_msginfo request) {
  (void)channel;
  (void)request;
  return microkit_msginfo_new(0, 0);
}
