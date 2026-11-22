-- The data-default facade package is not part of the freestanding closure;
-- its class is already supplied by the target-built data-default-class.
module Data.Default (Default (..)) where

import Data.Default.Class (Default (..))
