#ifndef NEXATRODE_SIMPLEX_SQLITE_INIT_H
#define NEXATRODE_SIMPLEX_SQLITE_INIT_H

#include "vault_storage.h"

/* Initialize from the app's Microkit setvar_vaddr=blob_transfer symbol and its
 * vault, entropy, and clock capabilities. Call once before opening the DB. */
int nex_simplex_sqlite_vfs_init_from_symbol(void);

/* Lower-level entry point for tests that provide the mapped page explicitly. */
int nex_simplex_sqlite_vfs_init(volatile NexVaultTransferPage *transfer);

#endif
