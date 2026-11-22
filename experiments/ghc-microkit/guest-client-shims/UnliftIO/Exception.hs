-- | Exception operations needed by the upstream TLS client and utilities.
module UnliftIO.Exception
  ( module E,
    catch,
    handle,
    bracket,
    bracket_,
    bracketOnError,
    finally,
    tryAny,
    handleAny,
    catchAny,
    Typeable,
  ) where

import Control.Exception as E hiding (bracket, bracketOnError, bracket_, catch, finally, handle)
import qualified Control.Exception as C
import Control.Monad.IO.Unlift (MonadUnliftIO, withRunInIO)
import Data.Typeable (Typeable)

catch :: (E.Exception e, MonadUnliftIO m) => m a -> (e -> m a) -> m a
catch action handler =
  withRunInIO $ \run -> C.catch (run action) (run . handler)

handle :: (E.Exception e, MonadUnliftIO m) => (e -> m a) -> m a -> m a
handle handler action = catch action handler

bracketOnError :: MonadUnliftIO m => m a -> (a -> m b) -> (a -> m c) -> m c
bracketOnError before after thing =
  withRunInIO $ \run -> C.bracketOnError (run before) (run . after) (run . thing)

bracket :: MonadUnliftIO m => m a -> (a -> m b) -> (a -> m c) -> m c
bracket before after thing =
  withRunInIO $ \run -> C.bracket (run before) (run . after) (run . thing)

bracket_ :: MonadUnliftIO m => m a -> m b -> m c -> m c
bracket_ before after thing =
  withRunInIO $ \run -> C.bracket_ (run before) (run after) (run thing)

finally :: MonadUnliftIO m => m a -> m b -> m a
finally action sequel = withRunInIO $ \run -> C.finally (run action) (run sequel)

tryAny :: MonadUnliftIO m => m a -> m (Either SomeException a)
tryAny action = withRunInIO $ \run -> C.try (run action)

handleAny :: MonadUnliftIO m => (SomeException -> m a) -> m a -> m a
handleAny handler action =
  withRunInIO $ \run -> C.catch (run action) (run . handler)

catchAny :: MonadUnliftIO m => m a -> (SomeException -> m a) -> m a
catchAny = catch
