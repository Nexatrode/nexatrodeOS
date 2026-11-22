{-# LANGUAGE NumericUnderscores #-}

module Network.HTTP2.Arch.Rate
  ( Rate,
    newRate,
    getRate,
  )
where

import Data.IORef
import GHC.Clock (getMonotonicTimeNSec)

newtype Rate = Rate (IORef Counter)

data Counter = Counter Int Integer

newRate :: IO Rate
newRate = do
  now <- getMonotonicTimeNSec
  Rate <$> newIORef (Counter 0 (toInteger now `div` 1_000_000_000))

getRate :: Rate -> IO Int
getRate (Rate ref) = do
  now <- getMonotonicTimeNSec
  let second = toInteger now `div` 1_000_000_000
  Counter count start <- readIORef ref
  if second - start > 1
    then writeIORef ref (Counter 1 second) >> pure 1
    else do
      let count' = count + 1
      writeIORef ref (Counter count' start)
      pure count'
