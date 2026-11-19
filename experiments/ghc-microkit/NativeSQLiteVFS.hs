-- | Register the vault-backed SQLCipher VFS before opening SimpleX AgentStore.
module NativeSQLiteVFS (registerVaultSQLiteVFS) where

import Foreign.C.Types (CInt (..))
foreign import ccall unsafe "nex_simplex_sqlite_vfs_init_from_symbol"
  c_registerVaultSQLiteVFS :: IO CInt

-- The Microkit loader fills the mapped BlobTransferPage linker variable. A
-- non-zero SQLite result means setup failed and the agent must not start.
registerVaultSQLiteVFS :: IO CInt
registerVaultSQLiteVFS = c_registerVaultSQLiteVFS
