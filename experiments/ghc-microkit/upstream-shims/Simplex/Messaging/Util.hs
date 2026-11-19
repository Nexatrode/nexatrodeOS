-- | Guest-only narrow dependency of upstream Encoding.hs. This definition is
-- identical to Simplex.Messaging.Util.(<$?>) in SimpleXMQ 7.0.0.6.
module Simplex.Messaging.Util ((<$?>)) where

(<$?>) :: MonadFail m => (a -> Either String b) -> m a -> m b
f <$?> m = either fail pure . f =<< m
