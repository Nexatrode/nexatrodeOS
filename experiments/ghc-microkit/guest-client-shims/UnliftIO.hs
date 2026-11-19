-- | Guest subset of UnliftIO used by the upstream client utility module.
-- These wrappers preserve async cancellation and run actions in the caller's
-- unlifting context; they do not provide filesystem or process operations.
module UnliftIO
  ( withAsync, waitAnyCancel, mapConcurrently, atomically, newTVarIO, newEmptyTMVarIO, readTVarIO, timeout, atomicModifyIORef', MonadIO,
    module Control.Concurrent.STM, Exception, SomeException, SomeAsyncException,
    fromException,
    Handle, IOMode (..), liftIO, withFile,
  ) where

import Control.Concurrent.Async (Async)
import qualified Control.Concurrent.Async as Async
import Control.Concurrent.STM hiding (atomically, newEmptyTMVarIO, newTVarIO, readTVarIO)
import qualified Control.Concurrent.STM as STM
import Control.Exception (Exception, SomeAsyncException, SomeException, fromException)
import Control.Monad.IO.Class (MonadIO, liftIO)
import Control.Monad.IO.Unlift (MonadUnliftIO, withRunInIO)
import Data.IORef (atomicModifyIORef')
import System.IO (Handle, IOMode (..))
import qualified System.IO as IO
import qualified System.Timeout as Timeout

withAsync :: MonadUnliftIO m => m a -> (Async a -> m b) -> m b
withAsync action callback =
  withRunInIO $ \run -> Async.withAsync (run action) (run . callback)

waitAnyCancel :: MonadUnliftIO m => [Async a] -> m (Async a, a)
waitAnyCancel = liftIO . Async.waitAnyCancel

mapConcurrently :: MonadUnliftIO m => (a -> m b) -> [a] -> m [b]
mapConcurrently action values =
  withRunInIO $ \run -> Async.mapConcurrently (run . action) values

atomically :: MonadIO m => STM.STM a -> m a
atomically = liftIO . STM.atomically

newTVarIO :: MonadIO m => a -> m (TVar a)
newTVarIO = atomically . STM.newTVar

newEmptyTMVarIO :: MonadIO m => m (TMVar a)
newEmptyTMVarIO = liftIO STM.newEmptyTMVarIO

readTVarIO :: MonadIO m => TVar a -> m a
readTVarIO = liftIO . STM.readTVarIO

timeout :: MonadUnliftIO m => Int -> m a -> m (Maybe a)
timeout microseconds action =
  withRunInIO $ \run -> Timeout.timeout microseconds (run action)

withFile :: MonadUnliftIO m => FilePath -> IOMode -> (Handle -> m a) -> m a
withFile path mode action =
  withRunInIO $ \run -> IO.withFile path mode (run . action)
