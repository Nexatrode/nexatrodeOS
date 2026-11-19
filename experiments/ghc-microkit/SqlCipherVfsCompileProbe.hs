{-# LANGUAGE OverloadedStrings #-}

module SqlCipherVfsCompileProbe (openSqlCipherVfs) where

import Data.IORef (newIORef)
import qualified Data.Text as T
import qualified Database.SQLite.Simple.Internal as SQLInternal
import qualified Database.SQLite3 as SQLite3

-- Mirrors the connection construction used by the SimpleX Microkit store
-- patch. This is a target compile probe; the actual VFS is tested in QEMU.
openSqlCipherVfs :: String -> String -> IO SQLInternal.Connection
openSqlCipherVfs path vfs = do
  database <-
    SQLite3.open2
      (T.pack path)
      [SQLite3.SQLOpenReadWrite, SQLite3.SQLOpenCreate, SQLite3.SQLOpenFullMutex]
      (SQLite3.SQLVFSCustom $ T.pack vfs)
  tempNameCounter <- newIORef 0
  pure $ SQLInternal.Connection database tempNameCounter
