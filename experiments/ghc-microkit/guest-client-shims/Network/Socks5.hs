-- | Data-only SOCKS5 credentials surface for the target client API.
--
-- SOCKS proxy dialing stays disabled in the Microkit transport backend; this
-- type is still carried through shared SimpleX configuration records.
module Network.Socks5 (SocksCredentials (..)) where

import Data.ByteString (ByteString)

data SocksCredentials = SocksCredentials ByteString ByteString
  deriving (Eq, Show)
