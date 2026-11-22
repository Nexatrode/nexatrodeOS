-- Arity-specialized function composition used by SimpleXMQ's source modules.
module Data.Composition ((.:), (.:.), (.::)) where

infixr 9 .:
infixr 9 .:.
infixr 9 .::

(.:) :: (c -> d) -> (a -> b -> c) -> a -> b -> d
(f .: g) a b = f (g a b)

(.:.) :: (d -> e) -> (a -> b -> c -> d) -> a -> b -> c -> e
(f .:. g) a b c = f (g a b c)

(.::) :: (e -> f) -> (a -> b -> c -> d -> e) -> a -> b -> c -> d -> f
(f .:: g) a b c d = f (g a b c d)
