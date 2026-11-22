-- | Timeout operation lifted over guest monads.
module UnliftIO.Timeout (timeout) where

import Control.Monad.IO.Unlift (MonadUnliftIO, withRunInIO)
import qualified System.Timeout as T

timeout :: MonadUnliftIO m => Int -> m a -> m (Maybe a)
timeout micros action = withRunInIO $ \run -> T.timeout micros (run action)
