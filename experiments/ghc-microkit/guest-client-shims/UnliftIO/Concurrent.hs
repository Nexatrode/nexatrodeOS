-- Small guest subset with UnliftIO's caller-context-preserving thread helpers.
module UnliftIO.Concurrent (module Control.Concurrent, forkIO, forkFinally, mkWeakThreadId) where

import Control.Concurrent hiding (forkFinally, forkIO, mkWeakThreadId)
import qualified Control.Concurrent as C
import Control.Monad.IO.Class (liftIO)
import Control.Monad.IO.Unlift (MonadUnliftIO, withRunInIO)
import Control.Exception (SomeException)
import System.Mem.Weak (Weak)

forkIO :: MonadUnliftIO m => m () -> m ThreadId
forkIO action = withRunInIO $ \run -> C.forkIO (run action)

forkFinally :: MonadUnliftIO m => m a -> (Either SomeException a -> m ()) -> m ThreadId
forkFinally action callback =
  withRunInIO $ \run -> C.forkFinally (run action) (run . callback)

mkWeakThreadId :: MonadUnliftIO m => ThreadId -> m (Weak ThreadId)
mkWeakThreadId = liftIO . C.mkWeakThreadId
