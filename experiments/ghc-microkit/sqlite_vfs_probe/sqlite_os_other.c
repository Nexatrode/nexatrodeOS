/* OS_OTHER hooks for the app-scoped Microkit VFS build. */
#include <sqlite3.h>

/* The application registers its capability-backed VFS before opening a DB.
 * There is no process-global default VFS or host filesystem to initialize. */
SQLITE_API int sqlite3_os_init(void) { return SQLITE_OK; }
SQLITE_API int sqlite3_os_end(void) { return SQLITE_OK; }
