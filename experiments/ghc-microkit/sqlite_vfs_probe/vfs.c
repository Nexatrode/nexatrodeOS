#include "vfs.h"

#include <limits.h>
#include <string.h>

typedef struct {
  sqlite3_file base;
  NexSqliteVfs *vfs;
  int file_id;
  int lock_level;
} NexFile;

static int io_close(sqlite3_file *file) {
  NexFile *f = (NexFile *)file;
  f->base.pMethods = NULL;
  return SQLITE_OK;
}

static int io_read(sqlite3_file *file, void *buffer, int amount,
                   sqlite3_int64 offset) {
  NexFile *f = (NexFile *)file;
  if (offset < 0 || amount < 0) return SQLITE_IOERR_READ;
  return f->vfs->ops->read(f->vfs->context, f->file_id, buffer, amount,
                           offset);
}

static int io_write(sqlite3_file *file, const void *buffer, int amount,
                    sqlite3_int64 offset) {
  NexFile *f = (NexFile *)file;
  if (offset < 0 || amount < 0) return SQLITE_IOERR_WRITE;
  return f->vfs->ops->write(f->vfs->context, f->file_id, buffer, amount,
                            offset);
}

static int io_truncate(sqlite3_file *file, sqlite3_int64 size) {
  NexFile *f = (NexFile *)file;
  if (size < 0) return SQLITE_IOERR_TRUNCATE;
  return f->vfs->ops->truncate(f->vfs->context, f->file_id, size);
}

static int io_sync(sqlite3_file *file, int flags) {
  NexFile *f = (NexFile *)file;
  (void)flags;
  return f->vfs->ops->sync(f->vfs->context, f->file_id);
}

static int io_file_size(sqlite3_file *file, sqlite3_int64 *size) {
  NexFile *f = (NexFile *)file;
  return f->vfs->ops->size(f->vfs->context, f->file_id, size);
}

static int io_lock(sqlite3_file *file, int requested) {
  NexFile *f = (NexFile *)file;
  if (requested <= f->lock_level) return SQLITE_OK;
  if (f->file_id == NEX_FILE_DB && f->vfs->lock_level > f->lock_level &&
      requested >= SQLITE_LOCK_SHARED)
    return SQLITE_BUSY;
  f->lock_level = requested;
  if (f->file_id == NEX_FILE_DB && requested > f->vfs->lock_level)
    f->vfs->lock_level = requested;
  return SQLITE_OK;
}

static int io_unlock(sqlite3_file *file, int requested) {
  NexFile *f = (NexFile *)file;
  f->lock_level = requested;
  if (f->file_id == NEX_FILE_DB) f->vfs->lock_level = requested;
  return SQLITE_OK;
}

static int io_check_reserved(sqlite3_file *file, int *reserved) {
  NexFile *f = (NexFile *)file;
  *reserved = f->vfs->lock_level >= SQLITE_LOCK_RESERVED;
  return SQLITE_OK;
}

static int io_file_control(sqlite3_file *file, int op, void *arg) {
  NexFile *f = (NexFile *)file;
  if (op == SQLITE_FCNTL_LOCKSTATE) {
    *(int *)arg = f->lock_level;
    return SQLITE_OK;
  }
  return SQLITE_NOTFOUND;
}

static int io_sector_size(sqlite3_file *file) {
  (void)file;
  return 4096;
}

static int io_device_characteristics(sqlite3_file *file) {
  (void)file;
  return 0; /* Do not claim atomic writes or safe append. */
}

static const sqlite3_io_methods io_methods = {
    .iVersion = 1,
    .xClose = io_close,
    .xRead = io_read,
    .xWrite = io_write,
    .xTruncate = io_truncate,
    .xSync = io_sync,
    .xFileSize = io_file_size,
    .xLock = io_lock,
    .xUnlock = io_unlock,
    .xCheckReservedLock = io_check_reserved,
    .xFileControl = io_file_control,
    .xSectorSize = io_sector_size,
    .xDeviceCharacteristics = io_device_characteristics,
};

static NexSqliteVfs *active_vfs;

static int has_suffix(const char *name, const char *suffix) {
  size_t name_length = strlen(name);
  size_t suffix_length = strlen(suffix);
  return name_length >= suffix_length &&
         memcmp(name + name_length - suffix_length, suffix, suffix_length) == 0;
}

static int vfs_open(sqlite3_vfs *vfs, const char *name, sqlite3_file *file,
                    int flags, int *out_flags) {
  NexFile *f = (NexFile *)file;
  int id;
  (void)vfs;
  if (flags & SQLITE_OPEN_MAIN_DB)
    id = NEX_FILE_DB;
  else if (flags & SQLITE_OPEN_MAIN_JOURNAL)
    id = NEX_FILE_JOURNAL;
  else
    return SQLITE_CANTOPEN;
  if (name == NULL) return SQLITE_CANTOPEN;
  memset(f, 0, sizeof(*f));
  f->vfs = active_vfs;
  f->file_id = id;
  f->base.pMethods = &io_methods;
  if (out_flags != NULL) *out_flags = flags;
  return SQLITE_OK;
}

static int vfs_delete(sqlite3_vfs *vfs, const char *name, int sync_dir) {
  int id, exists, rc;
  (void)vfs;
  (void)sync_dir;
  if (name == NULL) return SQLITE_IOERR_DELETE;
  if (has_suffix(name, "-journal"))
    id = NEX_FILE_JOURNAL;
  else if (has_suffix(name, "-wal"))
    return SQLITE_OK;
  else
    return SQLITE_IOERR_DELETE;
  rc = active_vfs->ops->delete_file(active_vfs->context, id);
  if (rc != SQLITE_OK) return rc;
  rc = active_vfs->ops->exists(active_vfs->context, id, &exists);
  return rc == SQLITE_OK && !exists ? SQLITE_OK : SQLITE_IOERR_DELETE;
}

static int vfs_access(sqlite3_vfs *vfs, const char *name, int flags,
                      int *result) {
  int id;
  (void)vfs;
  (void)flags;
  if (name == NULL || result == NULL) return SQLITE_IOERR_ACCESS;
  if (has_suffix(name, "-journal"))
    id = NEX_FILE_JOURNAL;
  else if (has_suffix(name, "-wal")) {
    *result = 0;
    return SQLITE_OK;
  }
  else
    id = NEX_FILE_DB;
  return active_vfs->ops->exists(active_vfs->context, id, result);
}

static int vfs_full_pathname(sqlite3_vfs *vfs, const char *name, int size,
                             char *out) {
  size_t length;
  (void)vfs;
  if (name == NULL || out == NULL || size <= 0) return SQLITE_CANTOPEN;
  length = strlen(name);
  if (length >= (size_t)size) return SQLITE_CANTOPEN;
  memcpy(out, name, length + 1);
  return SQLITE_OK;
}

static void *vfs_dl_open(sqlite3_vfs *vfs, const char *name) {
  (void)vfs;
  (void)name;
  return NULL;
}
static void vfs_dl_error(sqlite3_vfs *vfs, int size, char *message) {
  (void)vfs;
  if (size > 0) message[0] = '\0';
}
static void (*vfs_dl_sym(sqlite3_vfs *vfs, void *handle, const char *symbol))(
    void) {
  (void)vfs;
  (void)handle;
  (void)symbol;
  return NULL;
}
static void vfs_dl_close(sqlite3_vfs *vfs, void *handle) {
  (void)vfs;
  (void)handle;
}

static int vfs_randomness(sqlite3_vfs *vfs, int amount, char *buffer) {
  (void)vfs;
  return active_vfs->ops->randomness(active_vfs->context, amount, buffer);
}

static int vfs_sleep(sqlite3_vfs *vfs, int microseconds) {
  (void)vfs;
  (void)microseconds;
  return 0;
}

static int vfs_current_time(sqlite3_vfs *vfs, double *julian_day) {
  sqlite3_int64 millis;
  int rc;
  (void)vfs;
  rc = active_vfs->ops->current_time(active_vfs->context, &millis);
  if (rc == SQLITE_OK) *julian_day = (double)millis / 86400000.0;
  return rc;
}

static int vfs_current_time_int64(sqlite3_vfs *vfs, sqlite3_int64 *julian_ms) {
  (void)vfs;
  return active_vfs->ops->current_time(active_vfs->context, julian_ms);
}

static int vfs_last_error(sqlite3_vfs *vfs, int size, char *message) {
  (void)vfs;
  if (size > 0) message[0] = '\0';
  return 0;
}

int nex_sqlite_vfs_register(NexSqliteVfs *state, const NexStorageOps *ops,
                            void *context, const char *name) {
  static sqlite3_vfs vfs;
  if (state == NULL || ops == NULL || name == NULL || active_vfs != NULL)
    return SQLITE_MISUSE;
  if (ops->read == NULL || ops->write == NULL || ops->truncate == NULL ||
      ops->sync == NULL || ops->size == NULL || ops->delete_file == NULL ||
      ops->exists == NULL || ops->randomness == NULL ||
      ops->current_time == NULL)
    return SQLITE_MISUSE;
  state->ops = ops;
  state->context = context;
  state->lock_level = SQLITE_LOCK_NONE;
  active_vfs = state;
  memset(&vfs, 0, sizeof(vfs));
  vfs.iVersion = 2;
  vfs.szOsFile = sizeof(NexFile);
  vfs.mxPathname = 256;
  vfs.zName = name;
  vfs.xOpen = vfs_open;
  vfs.xDelete = vfs_delete;
  vfs.xAccess = vfs_access;
  vfs.xFullPathname = vfs_full_pathname;
  vfs.xDlOpen = vfs_dl_open;
  vfs.xDlError = vfs_dl_error;
  vfs.xDlSym = vfs_dl_sym;
  vfs.xDlClose = vfs_dl_close;
  vfs.xRandomness = vfs_randomness;
  vfs.xSleep = vfs_sleep;
  vfs.xCurrentTime = vfs_current_time;
  vfs.xGetLastError = vfs_last_error;
  vfs.xCurrentTimeInt64 = vfs_current_time_int64;
  /* This is also SQLite's source for startup entropy and current time. Make
   * it the default so those callbacks stay inside the Microkit capabilities,
   * including for sqlite3_randomness() calls made before a named-VFS open. */
  int rc = sqlite3_vfs_register(&vfs, 1);
  if (rc != SQLITE_OK) return rc;
  /* SQLCipher's provider requests strong entropy during SQLite startup. In
   * the freestanding build SQLITE_OMIT_AUTOINIT lets this VFS become default
   * first; initialize explicitly only after that capability route is live. */
  return sqlite3_initialize();
}
