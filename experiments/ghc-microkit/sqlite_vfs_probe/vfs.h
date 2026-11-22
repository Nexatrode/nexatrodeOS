#ifndef NEXATRODE_SQLITE_VFS_PROBE_H
#define NEXATRODE_SQLITE_VFS_PROBE_H

#include <sqlite3.h>
#include <stddef.h>
#include <stdint.h>

/* Storage is supplied by the caller, so target code can bridge these methods
 * to an app-scoped vault capability without adding POSIX file calls. */
typedef struct {
  int (*read)(void *context, int file_id, void *buffer, int amount,
              sqlite3_int64 offset);
  int (*write)(void *context, int file_id, const void *buffer, int amount,
               sqlite3_int64 offset);
  int (*truncate)(void *context, int file_id, sqlite3_int64 size);
  int (*sync)(void *context, int file_id);
  int (*size)(void *context, int file_id, sqlite3_int64 *size);
  int (*delete_file)(void *context, int file_id);
  int (*exists)(void *context, int file_id, int *exists);
  int (*randomness)(void *context, int amount, char *buffer);
  int (*current_time)(void *context, sqlite3_int64 *julian_ms);
} NexStorageOps;

enum { NEX_FILE_DB = 0, NEX_FILE_JOURNAL = 1, NEX_FILE_COUNT = 2 };

typedef struct {
  const NexStorageOps *ops;
  void *context;
  int lock_level;
} NexSqliteVfs;

int nex_sqlite_vfs_register(NexSqliteVfs *vfs, const NexStorageOps *ops,
                            void *context, const char *name);

#endif
