-- The agent uses only `async`; delegate to the target-built async package.
module UnliftIO.Async
  ( async,
    mapConcurrently,
    forConcurrently,
    concurrently_,
    race,
    raceAny_,
    uninterruptibleCancel,
  )
where

import Control.Concurrent.Async (Async, concurrently_, race)
import qualified Control.Concurrent.Async as A
import Control.Monad.IO.Unlift (MonadUnliftIO, withRunInIO)
import Control.Monad.IO.Class (MonadIO, liftIO)
import Control.Monad (void)

async :: MonadUnliftIO m => m a -> m (Async a)
async action = withRunInIO $ \run -> A.async (run action)

forConcurrently :: MonadUnliftIO m => [a] -> (a -> m b) -> m [b]
forConcurrently values action =
  withRunInIO $ \run -> A.forConcurrently values (run . action)

mapConcurrently :: MonadUnliftIO m => (a -> m b) -> [a] -> m [b]
mapConcurrently action values =
  withRunInIO $ \run -> A.mapConcurrently (run . action) values

uninterruptibleCancel :: MonadIO m => Async a -> m ()
uninterruptibleCancel = liftIO . A.uninterruptibleCancel

raceAny_ :: MonadUnliftIO m => [m a] -> m ()
raceAny_ actions = withRunInIO $ \run -> raceMany (map run actions)
  where
    raceMany [] = pure ()
    raceMany [action] = action >> pure ()
    raceMany (action : rest) = void $ A.race action (raceMany rest)
