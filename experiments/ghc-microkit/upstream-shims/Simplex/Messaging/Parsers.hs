-- | Guest-only narrow dependency of upstream Encoding.hs. This definition is
-- identical to Simplex.Messaging.Parsers.parseAll in SimpleXMQ 7.0.0.6.
module Simplex.Messaging.Parsers (parseAll) where

import Data.Attoparsec.ByteString.Char8 (Parser)
import qualified Data.Attoparsec.ByteString.Char8 as A
import Data.ByteString (ByteString)

parseAll :: Parser a -> ByteString -> Either String a
parseAll parser = A.parseOnly (parser <* A.endOfInput)
