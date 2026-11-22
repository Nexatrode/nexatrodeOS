-- | STM compatibility surface used by upstream HTTP/2 on the guest.
module UnliftIO.STM
  ( module STM,
    checkSTM,
    retrySTM,
    atomically,
    readTVarIO,
    registerDelay,
    newTVarIO,
    newEmptyTMVarIO,
  ) where

import Control.Concurrent.STM as STM hiding
  ( atomically,
    newEmptyTMVarIO,
    newTVarIO,
    readTVarIO,
    registerDelay
  )
import qualified Control.Concurrent.STM as T
import Control.Monad.IO.Class (MonadIO, liftIO)

checkSTM :: Bool -> STM ()
checkSTM = check

retrySTM :: STM a
retrySTM = retry

atomically :: MonadIO m => STM a -> m a
atomically = liftIO . T.atomically

readTVarIO :: MonadIO m => TVar a -> m a
readTVarIO = liftIO . T.readTVarIO

registerDelay :: MonadIO m => Int -> m (TVar Bool)
registerDelay = liftIO . T.registerDelay

newTVarIO :: MonadIO m => a -> m (TVar a)
newTVarIO = atomically . T.newTVar

newEmptyTMVarIO :: MonadIO m => m (TMVar a)
newEmptyTMVarIO = liftIO T.newEmptyTMVarIO
