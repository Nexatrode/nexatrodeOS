{-# LANGUAGE GADTs #-}
{-# LANGUAGE KindSignatures #-}

-- The dependency closure uses only the standard Dict proof carrier from the
-- constraints package. Keep this exact minimal definition available to the
-- freestanding target build until that package is cross-built for the guest.
module Data.Constraint (Dict (..)) where

import Data.Kind (Constraint)

data Dict (constraint :: Constraint) where
  Dict :: constraint => Dict constraint
