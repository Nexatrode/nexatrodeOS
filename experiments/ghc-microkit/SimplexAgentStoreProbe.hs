{-# LANGUAGE OverloadedStrings #-}

-- | Minimal upstream AgentStore startup entry point for a future Microkit
-- image. This is intentionally separate from the UI and from network startup.
module Main (main) where

import qualified Data.ByteArray as BA
import Data.ByteArray (ScrubbedBytes)
import qualified Data.ByteString.Char8 as BSC
import qualified Simplex.Messaging.Agent.Store.AgentStore as AgentStore
import Simplex.Messaging.Agent.Env.SQLite (createAgentStore)
import Simplex.Messaging.Agent.Store.SQLite (closeDBStore)
import Simplex.Messaging.Agent.Store.SQLite.Common (withTransaction)
import Simplex.Messaging.Agent.Store.SQLite.Common (DBOpts (..))
import Simplex.Messaging.Agent.Store.SQLite.DB (TrackQueries (TQOff))
import Simplex.Messaging.Agent.Store.Shared
  (MigrationConfig (..), MigrationConfirmation (MCYesUpDown))

main :: IO ()
main = do
  created <- createAgentStore
    DBOpts
      { dbFilePath = "agent.db",
        dbFunctions = [],
        -- Fixed test key only. A device build must obtain a fresh per-install
        -- key from the vault/key service rather than embedding one in the image.
        dbKey = BA.convert (BSC.pack "qemu-agentstore-test-key-v1") :: ScrubbedBytes,
        keepKey = True,
        vacuum = False,
        track = TQOff
      }
    MigrationConfig {confirm = MCYesUpDown, backupPath = Nothing}
  case created of
    Left err -> print err >> putStrLn "AGENTSTORE_MIGRATIONS|FAIL"
    Right store -> do
      putStrLn "AGENTSTORE_MIGRATIONS|PASS"
      existing <- withTransaction store AgentStore.getUserIds
      recordId <- case existing of
        userId : _ -> pure userId
        [] -> withTransaction store AgentStore.createUserRecord
      stored <- withTransaction store AgentStore.getUserIds
      if recordId `elem` stored
        then putStrLn ("AGENTSTORE_RECORD|PASS|" <> show recordId)
        else putStrLn "AGENTSTORE_RECORD|FAIL"
      closeDBStore store
