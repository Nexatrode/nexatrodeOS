{-# LANGUAGE DataKinds #-}
{-# LANGUAGE PatternSynonyms #-}

-- | Microkit client-only boundary for LAN discovery and the remote-control
-- TLS listener. These host-network features are intentionally unavailable;
-- callers fail closed instead of reaching POSIX sockets.
module Simplex.RemoteControl.Discovery
  ( pattern ANY_ADDR_V4,
    getLocalAddress,
    startTLSServer,
    withSender,
    withListener,
    recvAnnounce,
  )
where

import Control.Concurrent.Async (Async)
import Control.Concurrent.STM (TMVar)
import Control.Exception (throwIO)
import Data.ByteString (ByteString)
import Data.String (IsString)
import Data.Word (Word16)
import qualified Network.Socket as N
import qualified Network.TLS as TLS
import qualified Simplex.Messaging.Transport as Transport
import Simplex.RemoteControl.Types (RCCtrlAddress)

pattern ANY_ADDR_V4 :: (IsString a, Eq a) => a
pattern ANY_ADDR_V4 = "0.0.0.0"

data UDPSocket = UDPSocket
data ListenSocket = ListenSocket

unavailable :: IO a
unavailable = throwIO $ userError "Microkit remote-control discovery/listener service is unavailable"

getLocalAddress :: Maybe RCCtrlAddress -> IO [RCCtrlAddress]
getLocalAddress _ = pure []

startTLSServer
  :: Maybe Word16
  -> TMVar (Maybe N.PortNumber)
  -> TLS.Credential
  -> TLS.ServerHooks
  -> (Transport.TLS 'Transport.TServer -> IO ())
  -> IO (Async ())
startTLSServer _ _ _ _ _ = unavailable

withSender :: (UDPSocket -> IO a) -> IO a
withSender _ = unavailable

withListener :: TMVar Int -> (ListenSocket -> IO a) -> IO a
withListener _ _ = unavailable

recvAnnounce :: ListenSocket -> IO (N.SockAddr, ByteString)
recvAnnounce _ = unavailable
