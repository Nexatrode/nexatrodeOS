{-# LANGUAGE CPP #-}

-- | TLS I/O over the isolated network protection domain. The low-level
-- backend wraps an open session; the connector helpers resolve and open one.
-- TLS authentication and certificate validation remain the caller's responsibility.
module NativeTLSBackend
  ( nativeTLSBackend,
    nativeTLSBackendConnector,
    nativeTLSBackendEphemeralConnector,
    nativeTLSBackendForHost,
  ) where

import Control.Concurrent (threadDelay)
import Control.Exception (finally, onException)
import qualified Data.ByteString as B
import Data.Word (Word8, Word16)
import NativeNetwork
import qualified Network.TLS as TLS
import Text.Read (readMaybe)

-- | Adapter with the same host/service shape as SimpleXMQ's
-- getProtocolClientWithTLSBackend connector. DNS and TCP remain capabilities
-- provided by the isolated network PD; no POSIX socket is created here.
-- Uses long-lived SMP sessions via connectFreeSession.
nativeTLSBackendConnector :: String -> String -> IO TLS.Backend
nativeTLSBackendConnector hostname service =
  nativeTLSBackendWith connectFreeSession True hostname service 25000

-- | XFTP/HTTP short sessions on the ephemeral TCP slot (slot 1).
-- Uses bounded reads so a wedged XFTP server cannot pin the agent forever
-- the way SMP long-polls intentionally do. The ephemeral gate serializes
-- concurrent XFTP prepare/FPUT workers so they cannot cancel each other.
nativeTLSBackendEphemeralConnector :: String -> String -> IO TLS.Backend
nativeTLSBackendEphemeralConnector hostname service = do
  acquireEphemeralGate
  -- FPUT can stall while snd_buf drains; 25s was aborting mid-TLS-record and
  -- producing XFTP DIGEST failures on retry.
  backend <-
    nativeTLSBackendWith connectEphemeralSession False hostname service 180000
      `onException` releaseEphemeralGate
  pure backend
    { TLS.backendClose = do
        -- Let lwIP finish FIN/ACK before SMP reclaims slot 1; otherwise the
        -- secondary relay reconnect races and descr SEND times out.
        TLS.backendClose backend `finally` do
          threadDelay 500000
          releaseEphemeralGate
    }

-- | Variant for bounded probes that need a shorter network deadline.
nativeTLSBackendForHost :: String -> String -> Int -> IO TLS.Backend
nativeTLSBackendForHost hostname service timeoutMs =
  nativeTLSBackendWith connectFreeSession True hostname service timeoutMs

nativeTLSBackendWith
  :: (IPv4 -> Word16 -> Int -> IO (Either NetworkError Word8))
  -> Bool
  -> String
  -> String
  -> Int
  -> IO TLS.Backend
nativeTLSBackendWith openSession longPoll hostname service timeoutMs = do
  putStrLn ("NATIVE_TLS|BEGIN|" ++ hostname ++ "|" ++ service)
  port <- case readMaybe service :: Maybe Integer of
    Just value | value > 0 && value <= 65535 -> pure (fromIntegral value :: Word16)
    _ -> ioError (userError "invalid native TLS service port")
  resolved <- resolveIPv4 hostname timeoutMs
  address <- case resolved of
    Right address -> pure address
    Left err -> do
      putStrLn ("NATIVE_TLS|DNS|" ++ show err)
      networkFailure err
  connected <- openSession address port timeoutMs
  case connected of
    Right slot -> do
      epoch <- currentEpoch slot
      putStrLn ("NATIVE_TLS|OPEN|" ++ show slot ++ "|epoch=" ++ show epoch)
      pure (nativeTLSBackendOn slot epoch timeoutMs longPoll)
    Left err -> do
      putStrLn ("NATIVE_TLS|TCP|" ++ show err)
      networkFailure err
  where
    networkFailure err = ioError (userError ("native TLS connect: " ++ show err))

nativeTLSBackend :: Int -> TLS.Backend
nativeTLSBackend timeoutMs = nativeTLSBackendOn 0 0 timeoutMs True

nativeTLSBackendOn :: Word8 -> Word8 -> Int -> Bool -> TLS.Backend
nativeTLSBackendOn slot epoch timeoutMs longPoll = TLS.Backend
  { TLS.backendFlush = pure ()
  , TLS.backendClose = do
      -- Epoch-guarded so a displaced SMP backend cannot tear down the XFTP
      -- session that reclaimed its slot.
      result <- closeSessionOn slot epoch timeoutMs
      case result of
        Right () -> pure ()
        Left Closed -> pure ()
        Left _ -> cancelSessionEpoch slot epoch
  , TLS.backendSend = \bytes -> sendBytesOn slot epoch bytes timeoutMs >>= requireSuccess
  , TLS.backendRecv = \count ->
      if count < 0 then ioError (userError "negative TLS receive length")
      else if count == 0 then pure B.empty
      else do
        result <-
          if longPoll
            then receiveExactlyBytesForeverOn slot epoch count
            else receiveExactlyBytesOn slot epoch count timeoutMs
        case result of
          Right bytes -> pure bytes
          Left Closed -> pure B.empty
          Left err -> ioError (userError ("native TLS receive: " ++ show err))
  }
  where
    requireSuccess (Right ()) = pure ()
    requireSuccess (Left err) = ioError (userError ("native TLS send: " ++ show err))
