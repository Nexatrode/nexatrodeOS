{-# LANGUAGE OverloadedStrings #-}

-- | Re-open the QEMU test AgentStore with a deliberately incorrect key.
module Main (main) where

import qualified Data.ByteArray as BA
import Data.ByteArray (ScrubbedBytes)
import qualified Data.ByteString.Char8 as BSC
import Control.Exception (SomeException, try)
import Simplex.Messaging.Agent.Env.SQLite (createAgentStore)
import Simplex.Messaging.Agent.Store.SQLite (closeDBStore)
import Simplex.Messaging.Agent.Store.SQLite.Common (DBOpts (..))
import Simplex.Messaging.Agent.Store.SQLite.DB (TrackQueries (TQOff))
import Simplex.Messaging.Agent.Store.Shared
  (MigrationConfig (..), MigrationConfirmation (MCYesUpDown))

main :: IO ()
main = do
  outcome <- tryAny $ createAgentStore
    DBOpts
      { dbFilePath = "agent.db",
        dbFunctions = [],
        dbKey = BA.convert (BSC.pack "deliberately-wrong-agent-key") :: ScrubbedBytes,
        keepKey = True,
        vacuum = False,
        track = TQOff
      }
    MigrationConfig {confirm = MCYesUpDown, backupPath = Nothing}
  case outcome of
    Left _ -> putStrLn "AGENTSTORE_WRONG_KEY|REJECTED"
    Right (Left _) -> putStrLn "AGENTSTORE_WRONG_KEY|REJECTED"
    Right (Right store) -> do
      closeDBStore store
      putStrLn "AGENTSTORE_WRONG_KEY|ACCEPTED"

tryAny :: IO a -> IO (Either SomeException a)
tryAny = try
