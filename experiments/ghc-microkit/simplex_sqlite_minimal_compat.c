/*
 * Target-only SQLite binding shims for the minimal SimpleX AgentStore probe.
 * Query tracking is disabled there (TrackQueries = TQOff), and extension
 * loading must remain unavailable in a Microkit protection domain.
 */
#include <sqlite3.h>

int sqlite3_enable_load_extension(sqlite3 *db, int onoff)
{
    (void)db;
    (void)onoff;
    return SQLITE_ERROR;
}

void *sqlite3_trace(sqlite3 *db, void (*callback)(void *, const char *), void *context)
{
    (void)db;
    (void)callback;
    (void)context;
    return 0;
}
