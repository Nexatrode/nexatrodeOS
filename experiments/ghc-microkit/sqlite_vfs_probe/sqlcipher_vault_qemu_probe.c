/* SQLCipher integration against the real sDDF-backed vault and entropy PDs.
 * QEMU-only validation; the key and disk image are development test fixtures. */
#include "vault_storage.h"
#include "vault_storage_microkit.h"
#include "vfs.h"

#include <microkit.h>
#include <string.h>

enum {
  VAULT_CHANNEL = 6,
  ENTROPY_CHANNEL = 2,
  ENTROPY_REQUEST_BYTES = 0x401,
  ENTROPY_REPLY_OK = 0x400,
};

static NexVaultStorage storage;
static NexSqliteVfs vfs_state;
static int entropy_ipc_failed;
uintptr_t messages_transfer;
/* The shared storage metaprogram also maps the Notes-transfer symbol. */
uintptr_t blob_transfer;

static int vault_randomness(void *context, int amount, char *buffer) {
  (void)context;
  int filled = 0;
  if (amount < 0 || (amount > 0 && buffer == NULL)) return 0;
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

static int test_current_time(void *context, sqlite3_int64 *julian_ms) {
  (void)context;
  if (julian_ms == NULL) return SQLITE_IOERR;
  *julian_ms = 212000000000000LL;
  return SQLITE_OK;
}

static void fail(const char *reason, int rc) {
  (void)rc;
  if (entropy_ipc_failed)
    microkit_dbg_puts("SQLCIPHER_VAULT|FAIL_ENTROPY_IPC\n");
  else if (reason[0] == 'i')
    microkit_dbg_puts("SQLCIPHER_VAULT|FAIL_INIT\n");
  else if (reason[0] == 'o')
    microkit_dbg_puts("SQLCIPHER_VAULT|FAIL_OPEN\n");
  else if (reason[0] == 'k')
    microkit_dbg_puts("SQLCIPHER_VAULT|FAIL_KEY\n");
  else if (reason[0] == 'c')
    microkit_dbg_puts("SQLCIPHER_VAULT|FAIL_CREATE\n");
  else if (reason[0] == 'w')
    microkit_dbg_puts("SQLCIPHER_VAULT|FAIL_WRONG_KEY_ACCEPTED\n");
  else
    microkit_dbg_puts("SQLCIPHER_VAULT|FAIL_READBACK\n");
  for (;;) {}
}

static sqlite3 *open_keyed(const char *key) {
  sqlite3 *db = NULL;
  int rc = sqlite3_open_v2("messages.db", &db,
                           SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
                           "nexatrode-storage");
  if (rc != SQLITE_OK) fail("open", rc);
  rc = sqlite3_key(db, key, (int)strlen(key));
  if (rc != SQLITE_OK) fail("key", rc);
  return db;
}

void init(void) {
  static const char test_key[] = "qemu-vault-integration-key";
  sqlite3 *db;
  sqlite3_stmt *stmt = NULL;
  sqlite3_int64 db_size = 0;
  int rc;

  if (messages_transfer == 0) fail("init-transfer", SQLITE_CANTOPEN);
  rc = nex_vault_storage_init(
      &storage, VAULT_CHANNEL,
      (volatile NexVaultTransferPage *)messages_transfer,
      nex_microkit_vault_call, NULL, vault_randomness, test_current_time, NULL);
  if (rc != SQLITE_OK) fail("init-vault", rc);
  rc = nex_sqlite_vfs_register(&vfs_state, nex_vault_storage_ops(), &storage,
                               "nexatrode-storage");
  if (rc != SQLITE_OK) fail("init-vfs", rc);

  rc = nex_vault_storage_ops()->size(&storage, NEX_FILE_DB, &db_size);
  if (rc != SQLITE_OK) fail("init-size", rc);
  db = open_keyed(test_key);
  if (db_size == 0) {
    rc = sqlite3_exec(db,
                      "PRAGMA journal_mode=DELETE;"
                      "CREATE TABLE messages(body TEXT NOT NULL);"
                      "INSERT INTO messages VALUES('vault survived reboot');",
                      NULL, NULL, NULL);
    if (rc != SQLITE_OK) fail("create-message", rc);
    rc = sqlite3_close(db);
    if (rc != SQLITE_OK) fail("create-close", rc);
    microkit_dbg_puts("SQLCIPHER_VAULT|CREATED_ENCRYPTED_DB\n");
  } else {
    rc = sqlite3_prepare_v2(db, "SELECT body FROM messages", -1, &stmt, NULL);
    if (rc != SQLITE_OK) fail("readback-prepare", rc);
    rc = sqlite3_step(stmt);
    if (rc != SQLITE_ROW ||
        strcmp((const char *)sqlite3_column_text(stmt, 0),
               "vault survived reboot") != 0)
      fail("readback-row", rc);
    sqlite3_finalize(stmt);
    stmt = NULL;
    rc = sqlite3_close(db);
    if (rc != SQLITE_OK) fail("readback-close", rc);
    microkit_dbg_puts("SQLCIPHER_VAULT|REOPENED_AFTER_REBOOT\n");
  }

  db = open_keyed("qemu-vault-wrong-key");
  rc = sqlite3_prepare_v2(db, "SELECT body FROM messages", -1, &stmt, NULL);
  if (rc == SQLITE_OK) {
    rc = sqlite3_step(stmt);
    if (rc == SQLITE_ROW) fail("wrong-key", rc);
    sqlite3_finalize(stmt);
  }
  sqlite3_close(db);
  microkit_dbg_puts("SQLCIPHER_VAULT|WRONG_KEY_REJECTED\n");
}

void notified(microkit_channel channel) { (void)channel; }

microkit_msginfo protected(microkit_channel channel,
                           microkit_msginfo request) {
  (void)channel;
  (void)request;
  return microkit_msginfo_new(0, 0);
}
