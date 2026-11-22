-- The minimal unlifted MVar operations used by the SQLite store.
module UnliftIO.MVar (MVar, withMVar) where

import Control.Concurrent.MVar (MVar)
import qualified Control.Concurrent.MVar as M
import Control.Monad.IO.Unlift (MonadUnliftIO, withRunInIO)

withMVar :: MonadUnliftIO m => MVar a -> (a -> m b) -> m b
withMVar var action = withRunInIO $ \run -> M.withMVar var (run . action)
