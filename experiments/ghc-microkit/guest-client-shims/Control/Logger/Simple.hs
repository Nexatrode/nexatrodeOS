-- | Guest logging adapter for the upstream modules used by the SMP client.
-- The GHC guest's stdout is routed to the Microkit console.
module Control.Logger.Simple (logDebug, logError, logInfo, logWarn) where

import Control.Monad.IO.Class (MonadIO, liftIO)
import Data.Text (Text)
import qualified Data.Text as Text

logDebug :: MonadIO m => Text -> m ()
logDebug = liftIO . putStrLn . ("DEBUG: " ++) . Text.unpack

logError :: MonadIO m => Text -> m ()
logError = liftIO . putStrLn . ("ERROR: " ++) . Text.unpack

logInfo :: MonadIO m => Text -> m ()
logInfo = liftIO . putStrLn . ("INFO: " ++) . Text.unpack

logWarn :: MonadIO m => Text -> m ()
logWarn = liftIO . putStrLn . ("WARN: " ++) . Text.unpack
