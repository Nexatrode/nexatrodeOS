#define _POSIX_C_SOURCE 200809L
#include "vfs.h"

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define FILE_CAPACITY (1024 * 1024)

typedef struct {
  unsigned char bytes[NEX_FILE_COUNT][FILE_CAPACITY];
  sqlite3_int64 size[NEX_FILE_COUNT];
  int exists[NEX_FILE_COUNT];
  int randomness_calls;
} MemoryStore;

static int store_read(void *ctx, int id, void *buffer, int amount,
                      sqlite3_int64 offset) {
  MemoryStore *s = ctx;
  sqlite3_int64 available = s->size[id] - offset;
  if (offset >= s->size[id]) {
    memset(buffer, 0, (size_t)amount);
    return SQLITE_IOERR_SHORT_READ;
  }
  if (available < amount) {
    memcpy(buffer, s->bytes[id] + offset, (size_t)available);
    memset((unsigned char *)buffer + available, 0, (size_t)(amount - available));
    return SQLITE_IOERR_SHORT_READ;
  }
  memcpy(buffer, s->bytes[id] + offset, (size_t)amount);
  return SQLITE_OK;
}

static int store_write(void *ctx, int id, const void *buffer, int amount,
                       sqlite3_int64 offset) {
  MemoryStore *s = ctx;
  sqlite3_int64 end = offset + amount;
  if (offset < 0 || end < offset || end > FILE_CAPACITY) return SQLITE_FULL;
  memcpy(s->bytes[id] + offset, buffer, (size_t)amount);
  if (end > s->size[id]) s->size[id] = end;
  s->exists[id] = 1;
  return SQLITE_OK;
}

static int store_truncate(void *ctx, int id, sqlite3_int64 size) {
  MemoryStore *s = ctx;
  if (size < 0 || size > FILE_CAPACITY) return SQLITE_FULL;
  if (size > s->size[id]) memset(s->bytes[id] + s->size[id], 0, (size_t)(size - s->size[id]));
  if (size < s->size[id]) memset(s->bytes[id] + size, 0, (size_t)(s->size[id] - size));
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
  MemoryStore *s = ctx;
  *size = s->size[id];
  return SQLITE_OK;
}

static int store_delete(void *ctx, int id) {
  MemoryStore *s = ctx;
  memset(s->bytes[id], 0, sizeof(s->bytes[id]));
  s->size[id] = 0;
  s->exists[id] = 0;
  return SQLITE_OK;
}

static int store_exists(void *ctx, int id, int *exists) {
  MemoryStore *s = ctx;
  *exists = s->exists[id];
  return SQLITE_OK;
}

static int store_randomness(void *ctx, int amount, char *buffer) {
  MemoryStore *s = ctx;
  s->randomness_calls++;
  for (int i = 0; i < amount; i++) buffer[i] = (char)rand();
  return amount;
}

static int store_time(void *ctx, sqlite3_int64 *julian_ms) {
  struct timespec now;
  (void)ctx;
  if (clock_gettime(CLOCK_REALTIME, &now) != 0) return SQLITE_IOERR;
  *julian_ms = 210866760000000LL + (sqlite3_int64)now.tv_sec * 1000 + now.tv_nsec / 1000000;
  return SQLITE_OK;
}

static const NexStorageOps ops = {
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

static void check_sql(int rc, sqlite3 *db, const char *what) {
  if (rc != SQLITE_OK) {
    fprintf(stderr, "%s: %s (%d)\n", what,
            db != NULL ? sqlite3_errmsg(db) : sqlite3_errstr(rc), rc);
    exit(1);
  }
}

static sqlite3 *open_db(NexSqliteVfs *vfs_state, const char *key) {
  sqlite3 *db = NULL;
  int rc = sqlite3_open_v2("agent.db", &db,
                           SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                           "nexatrode-storage");
  check_sql(rc, db, "sqlite3_open_v2");
#ifdef NEXATRODE_TEST_SQLCIPHER
  check_sql(sqlite3_key(db, key, (int)strlen(key)), db, "sqlite3_key");
#else
  (void)key;
#endif
  (void)vfs_state;
  return db;
}

int main(void) {
  static MemoryStore store;
  static NexSqliteVfs vfs_state;
  sqlite3 *db;
  sqlite3_stmt *stmt = NULL;
  int rc;

  srand(1234); /* Deterministic test entropy only; target uses the entropy PD. */
  rc = nex_sqlite_vfs_register(&vfs_state, &ops, &store, "nexatrode-storage");
  check_sql(rc, NULL, "register VFS");
  assert(sqlite3_vfs_find(NULL) != NULL);
  assert(strcmp(sqlite3_vfs_find(NULL)->zName, "nexatrode-storage") == 0);
  {
    unsigned char sample[16];
    assert(sqlite3_vfs_find(NULL)->xRandomness(
               sqlite3_vfs_find(NULL), sizeof(sample), (char *)sample) ==
           (int)sizeof(sample));
    assert(store.randomness_calls > 0);
  }
  db = open_db(&vfs_state, "test database key");
  check_sql(sqlite3_exec(db, "PRAGMA journal_mode=DELETE;"
                             "CREATE TABLE messages(id INTEGER PRIMARY KEY, body TEXT NOT NULL);"
                             "INSERT INTO messages(body) VALUES('persist across reopen');",
                         NULL, NULL, NULL), db, "create and insert");
  check_sql(sqlite3_close(db), NULL, "close first connection");

#ifdef NEXATRODE_TEST_SQLCIPHER
  assert(store.size[NEX_FILE_DB] >= 16);
  assert(memcmp(store.bytes[NEX_FILE_DB], "SQLite format 3", 15) != 0);

  db = open_db(&vfs_state, "wrong database key");
  rc = sqlite3_prepare_v2(db, "SELECT body FROM messages WHERE id=1", -1,
                          &stmt, NULL);
  if (rc == SQLITE_OK) {
    assert(sqlite3_step(stmt) != SQLITE_ROW);
    (void)sqlite3_finalize(stmt);
  }
  check_sql(sqlite3_close(db), NULL, "close wrong-key connection");
#endif

  db = open_db(&vfs_state, "test database key");
  check_sql(sqlite3_prepare_v2(db, "SELECT body FROM messages WHERE id=1", -1,
                               &stmt, NULL), db, "prepare readback");
  rc = sqlite3_step(stmt);
  assert(rc == SQLITE_ROW);
  assert(strcmp((const char *)sqlite3_column_text(stmt, 0),
                "persist across reopen") == 0);
  assert(sqlite3_step(stmt) == SQLITE_DONE);
  check_sql(sqlite3_finalize(stmt), db, "finalize readback");

  /* A storage capacity failure must not corrupt the last committed row. */
  {
    char *error = NULL;
    rc = sqlite3_exec(db,
                      "BEGIN; INSERT INTO messages(body) VALUES(zeroblob(1100000)); COMMIT;",
                      NULL, NULL, &error);
    assert(rc == SQLITE_FULL);
    sqlite3_free(error);
  }
  check_sql(sqlite3_close(db), NULL, "close reopened connection");

  db = open_db(&vfs_state, "test database key");
  check_sql(sqlite3_prepare_v2(db, "SELECT count(*) FROM messages", -1,
                               &stmt, NULL), db, "prepare count");
  assert(sqlite3_step(stmt) == SQLITE_ROW);
  assert(sqlite3_column_int(stmt, 0) == 1);
  check_sql(sqlite3_finalize(stmt), db, "finalize count");
  check_sql(sqlite3_close(db), NULL, "close after capacity failure");
#ifdef NEXATRODE_TEST_SQLCIPHER
  puts("SQLCIPHER_VFS|PASS|encrypted header, wrong-key rejection and reopen");
#else
  puts("SQLITE_VFS|PASS|reopen and capacity failure preserved committed row");
#endif
  return 0;
}
