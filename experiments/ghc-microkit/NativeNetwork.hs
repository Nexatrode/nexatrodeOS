{-# LANGUAGE CPP #-}
-- | Bounded, single-session networking through the isolated Microkit network PD.
module NativeNetwork
  ( IPv4(..), NetworkError(..), resolveIPv4, connectIPv4, connectFreeSession,
    connectEphemeralSession, currentEpoch,
    acquireEphemeralGate, releaseEphemeralGate, tryAcquireEphemeralGate,
    sendBytes, sendBytesOn, receiveSomeBytes, receiveExactlyBytes,
    receiveExactlyBytesOn, receiveExactlyBytesForever, receiveExactlyBytesForeverOn,
    sendAll, receiveSome, receiveExactly, closeSession, closeSessionOn, cancelSession,
    cancelSessionOn, cancelSessionEpoch
  ) where

import Control.Concurrent (threadDelay)
import Control.Concurrent.MVar (MVar, modifyMVar, newMVar, putMVar, readMVar, takeMVar, tryTakeMVar, withMVar)
import Control.Exception (mask, onException)
import Control.Monad (when)
import System.IO.Unsafe (unsafePerformIO)
import qualified Data.ByteString as B
import Data.Word (Word8, Word16, Word64)
import Foreign.C.Types (CInt(..))
import Foreign.Marshal.Alloc (allocaBytes)
import Foreign.Marshal.Array (peekArray, withArray)
import Foreign.Ptr (Ptr, castPtr, plusPtr)
import GHC.Clock (getMonotonicTimeNSec)

data IPv4 = IPv4 !Word8 !Word8 !Word8 !Word8 deriving (Eq, Show)
data NetworkError = Unavailable | InvalidInput | TimedOut | Closed | Busy | TransportError
  deriving (Eq, Show)

-- The two TCP sessions share one request page. Hold this only around a call.
pageLock :: MVar ()
pageLock = unsafePerformIO (newMVar ())
{-# NOINLINE pageLock #-}

-- Only one live ephemeral TLS session may own slot 1. XFTP prepare/FPUT open
-- several servers; without this gate connectEphemeralSession cancels the
-- previous occupant mid-handshake and uploads land incomplete.
ephemeralGate :: MVar ()
ephemeralGate = unsafePerformIO (newMVar ())
{-# NOINLINE ephemeralGate #-}

acquireEphemeralGate :: IO ()
acquireEphemeralGate = takeMVar ephemeralGate

releaseEphemeralGate :: IO ()
releaseEphemeralGate = putMVar ephemeralGate ()

-- | True when no ephemeral XFTP/HTTP session currently owns slot 1.
tryAcquireEphemeralGate :: IO Bool
tryAcquireEphemeralGate = do
  taken <- tryTakeMVar ephemeralGate
  case taken of
    Nothing -> pure False
    Just () -> putMVar ephemeralGate () >> pure True

-- Generation per slot so a stolen ephemeral session cannot be written by a
-- stale SMP TLS backend that still holds the old Word8 slot id.
slotEpochs :: MVar (Word8, Word8)
slotEpochs = unsafePerformIO (newMVar (0, 0))
{-# NOINLINE slotEpochs #-}

bumpEpoch :: Word8 -> IO Word8
bumpEpoch slot = modifyMVar slotEpochs $ \(e0, e1) ->
  let next = if slot == 0 then (e0 + 1, e1) else (e0, e1 + 1)
      cur = if slot == 0 then fst next else snd next
   in pure (next, cur)

currentEpoch :: Word8 -> IO Word8
currentEpoch slot = do
  (e0, e1) <- readMVar slotEpochs
  pure (if slot == 0 then e0 else e1)

locked :: IO a -> IO a
locked action = withMVar pageLock (\_ -> action)

foreign import ccall unsafe "sel4_haskell_dns_start"
    dnsStart :: Ptr Word8 -> CInt -> IO CInt
foreign import ccall unsafe "sel4_haskell_dns_poll"
    dnsPoll :: Ptr Word8 -> IO CInt
foreign import ccall unsafe "sel4_haskell_tcp_open"
    tcpOpen :: Word8 -> Word8 -> Word8 -> Word8 -> Word16 -> Word8 -> IO CInt
foreign import ccall unsafe "sel4_haskell_tcp_status"
    tcpStatus :: Word8 -> IO CInt
foreign import ccall unsafe "sel4_haskell_tcp_send"
    tcpSend :: Ptr Word8 -> CInt -> Word8 -> IO CInt
foreign import ccall unsafe "sel4_haskell_tcp_recv"
    tcpRecv :: Ptr Word8 -> CInt -> Word8 -> IO CInt
foreign import ccall unsafe "sel4_haskell_tcp_close"
    tcpClose :: Word8 -> IO CInt
foreign import ccall unsafe "sel4_haskell_tcp_cancel"
    tcpCancel :: Word8 -> IO ()

deadlineAfter :: Int -> IO Word64
deadlineAfter milliseconds = do
  now <- getMonotonicTimeNSec
  pure (now + fromIntegral (max 0 milliseconds) * 1000000)

retryPending :: Word64 -> IO (Either NetworkError a) -> IO (Either NetworkError a)
retryPending deadline action = do
  result <- action
  case result of
    Left TimedOut -> do
      now <- getMonotonicTimeNSec
      if now >= deadline then pure (Left TimedOut)
        else threadDelay 10000 >> retryPending deadline action
    _ -> pure result

resolveIPv4 :: String -> Int -> IO (Either NetworkError IPv4)
resolveIPv4 hostname timeoutMs
  | null hostname || length hostname >= 128 ||
    any (\c -> fromEnum c < 33 || fromEnum c > 126) hostname =
      pure (Left InvalidInput)
  | otherwise = withArray bytes $ \name -> allocaBytes 4 $ \address -> do
      deadline <- deadlineAfter timeoutMs
      started <- retryPending deadline $ do
        result <- locked $ dnsStart name (fromIntegral (length bytes))
        pure $ case result of
          1 -> Right ()
          0 -> Left TimedOut
          -2 -> Left Unavailable
          _ -> Left TransportError
      case started of
        Left err -> pure (Left err)
        Right () -> retryPending deadline $ do
          result <- locked $ dnsPoll address
          case result of
            4 -> do
              octets <- peekArray 4 address
              pure $ case octets of
                [a,b,c,d] -> Right (IPv4 a b c d)
                _ -> Left TransportError
            0 -> pure (Left TimedOut)
            _ -> pure (Left TransportError)
  where
    bytes = map (fromIntegral . fromEnum) hostname :: [Word8]

connectIPv4 :: IPv4 -> Word16 -> Int -> IO (Either NetworkError ())
connectIPv4 address port timeoutMs = fmap (const ()) <$> connectSlot 0 address port timeoutMs

-- | Open whichever session is free for long-lived SMP links. Prefer slot 0 so
-- slot 1 stays available for XFTP; fall back to slot 1 when both relays need a
-- socket. XFTP must use connectEphemeralSession, which reclaims slot 1.
connectFreeSession :: IPv4 -> Word16 -> Int -> IO (Either NetworkError Word8)
connectFreeSession address port timeoutMs = do
  deadline <- deadlineAfter timeoutMs
  let go = do
        opened0 <- connectSlot 0 address port timeoutMs
        case opened0 of
          Right _epoch -> pure (Right 0)
          -- Primary SMP often already owns slot 0. The network PD may surface
          -- that as Busy, TimedOut, or TransportError — still try slot 1 so a
          -- secondary relay can reconnect after ephemeral XFTP.
          Left err0
            | err0 == Unavailable -> pure (Left Unavailable)
            | otherwise -> do
                opened1 <- connectSlot 1 address port timeoutMs
                case opened1 of
                  Right _epoch -> pure (Right 1)
                  Left Busy -> do
                    threadDelay 50000
                    now <- getMonotonicTimeNSec
                    if now >= deadline
                      then pure (Left TimedOut)
                      else go
                  Left TimedOut -> do
                    threadDelay 50000
                    now <- getMonotonicTimeNSec
                    if now >= deadline
                      then pure (Left TimedOut)
                      else go
                  Left err -> pure (Left err)
  go

-- | Open the ephemeral session (slot 1) for XFTP/HTTP. Cancels any prior
-- occupant (including a secondary SMP link). That SMP backend sees Closed and
-- reconnects after XFTP finishes; do not share slot 1 across live TLS backends.
connectEphemeralSession :: IPv4 -> Word16 -> Int -> IO (Either NetworkError Word8)
connectEphemeralSession address port timeoutMs = do
  putStrLn "NATIVE_TCP|EPHEMERAL|1"
  cancelSessionOn 1
  -- Give a displaced SMP TLS backend time to observe Closed and exit its
  -- recv loop before we reuse the slot; its close is epoch-guarded.
  threadDelay 150000
  opened <- connectSlot 1 address port timeoutMs
  case opened of
    Right _epoch -> pure (Right 1)
    Left Busy -> do
      cancelSessionOn 1
      threadDelay 150000
      opened' <- connectSlot 1 address port timeoutMs
      case opened' of
        Right _epoch -> pure (Right 1)
        Left err -> pure (Left err)
    Left err -> pure (Left err)

-- | Returns the slot epoch stamped at connect time. Callers must pass that
-- epoch into send/recv so a stolen session cannot be used by a stale backend.
connectSlot :: Word8 -> IPv4 -> Word16 -> Int -> IO (Either NetworkError Word8)
connectSlot slot (IPv4 a b c d) port timeoutMs
  | port == 0 = pure (Left InvalidInput)
  | otherwise = mask $ \restore -> do
      deadline <- deadlineAfter timeoutMs
      started <- retryPending deadline $ locked $ do
        result <- tcpOpen a b c d port slot
        case result of
          1 -> Right <$> bumpEpoch slot
          0 -> pure (Left TimedOut)
          2 -> pure (Left Busy)
          -2 -> pure (Left Unavailable)
          _ -> pure (Left TransportError)
      case started of
        Left err -> pure (Left err)
        Right epoch -> do
          let waitForConnection = retryPending deadline $ locked $ do
                alive <- guardEpoch slot epoch
                case alive of
                  Left err -> pure (Left err)
                  Right () -> do
                    result <- tcpStatus slot
                    pure $ case result of
                      1 -> Right epoch
                      0 -> Left TimedOut
                      -2 -> Left Closed
                      _ -> Left TransportError
          connected <- restore waitForConnection `onException` cancelSessionEpoch slot epoch
          case connected of
            Right _ -> pure connected
            Left _ -> cancelSessionEpoch slot epoch >> pure connected

guardEpoch :: Word8 -> Word8 -> IO (Either NetworkError ())
guardEpoch slot epoch = do
  cur <- currentEpoch slot
  pure $ if cur == epoch then Right () else Left Closed

-- | Send the entire payload, retrying backpressure until one overall deadline.
sendAll :: [Word8] -> Int -> IO (Either NetworkError ())
sendAll payload = sendBytes (B.pack payload)

-- | Strict bytestring send for protocol and TLS record callers.
sendBytes :: B.ByteString -> Int -> IO (Either NetworkError ())
sendBytes payload timeoutMs = sendBytesOn 0 0 payload timeoutMs

sendBytesOn :: Word8 -> Word8 -> B.ByteString -> Int -> IO (Either NetworkError ())
sendBytesOn slot epoch payload timeoutMs = B.useAsCStringLen payload $ \(dataPtr, total) -> do
  deadline <- deadlineAfter timeoutMs
  -- lwIP rejects tcp_write when len > pcb->snd_buf. Prefer 32-byte XFTP
  -- writes so the shared TCP page lock is not held in a 1-byte hot loop that
  -- starves SMP joins. On BUSY, pump lwIP (ACKs) then retry; shrink only
  -- after repeated backpressure. Slot 0 stays larger for SMP.
  go deadline (castPtr dataPtr) total 0 (if slot == 1 then 32 else 48)
  where
    pump = do
      _ <- locked $ tcpStatus slot
      pure ()
    go _ _ total offset _ | offset == total = pure (Right ())
    go deadline buffer total offset chunk = do
      now <- getMonotonicTimeNSec
      if now >= deadline
        then pure (Left TimedOut)
        else do
          let count = min chunk (total - offset)
          alive <- guardEpoch slot epoch
          case alive of
            Left err -> pure (Left err)
            Right () -> do
              result <- locked $ tcpSend (buffer `plusPtr` offset) (fromIntegral count) slot
              if result == fromIntegral count
                then do
                  when (slot == 1 && (offset == 0 || offset + count == total)) $
                    putStrLn ("NATIVE_TCP|SEND_OK|1|" <> show (offset + count) <> "/" <> show total)
                  go deadline buffer total (offset + count) (if slot == 1 then 32 else 48)
                else if result == 0
                  then do
                    when (slot == 1) $
                      putStrLn ("NATIVE_TCP|SEND_BUSY|1|off=" <> show offset <> "|chunk=" <> show chunk)
                    pump
                    threadDelay 15000
                    let chunk' = if slot == 1 && chunk > 1 then max 1 (chunk `div` 2) else chunk
                    go deadline buffer total offset chunk'
                  else pure (Left TransportError)

-- | Return one available chunk, or TimedOut if no bytes arrive by the deadline.
receiveSome :: Int -> Int -> IO (Either NetworkError [Word8])
receiveSome capacity timeoutMs = fmap B.unpack <$> receiveSomeBytes capacity timeoutMs

receiveSomeBytes :: Int -> Int -> IO (Either NetworkError B.ByteString)
receiveSomeBytes capacity timeoutMs
  | capacity <= 0 || capacity > 128 = pure (Left InvalidInput)
  | otherwise = allocaBytes capacity $ \buffer -> do
      deadline <- deadlineAfter timeoutMs
      receiveSomeBytesUntil buffer capacity deadline

receiveSomeBytesUntil :: Ptr Word8 -> Int -> Word64 -> IO (Either NetworkError B.ByteString)
receiveSomeBytesUntil buffer capacity deadline =
  receiveSomeBytesUntilOn 0 0 buffer capacity deadline

receiveSomeBytesUntilOn :: Word8 -> Word8 -> Ptr Word8 -> Int -> Word64 -> IO (Either NetworkError B.ByteString)
receiveSomeBytesUntilOn slot epoch buffer capacity deadline = retryPending deadline $ do
  alive <- guardEpoch slot epoch
  case alive of
    Left err -> pure (Left err)
    Right () -> do
      result <- locked $ tcpRecv buffer (fromIntegral capacity) slot
      if result > 0 && result <= fromIntegral capacity
        then Right <$> B.packCStringLen (castPtr buffer, fromIntegral result)
        else pure $ case result of
          0 -> Left TimedOut
          -2 -> Left Closed
          _ -> Left TransportError

-- | Read a known length without quadratic list appends or per-chunk timeouts.
receiveExactly :: Int -> Int -> IO (Either NetworkError [Word8])
receiveExactly total timeoutMs = fmap B.unpack <$> receiveExactlyBytes total timeoutMs

-- | TLS-compatible exact read: a single overall deadline and short reads joined.
receiveExactlyBytes :: Int -> Int -> IO (Either NetworkError B.ByteString)
receiveExactlyBytes total timeoutMs = receiveExactlyBytesOn 0 0 total timeoutMs

receiveExactlyBytesOn :: Word8 -> Word8 -> Int -> Int -> IO (Either NetworkError B.ByteString)
receiveExactlyBytesOn slot epoch total timeoutMs
  | total < 0 = pure (Left InvalidInput)
  | otherwise = allocaBytes 64 $ \buffer -> do
      deadline <- deadlineAfter timeoutMs
      go buffer deadline 0 []
  where
    go _ _ count reversed | count == total = pure (Right (B.concat (reverse reversed)))
    go buffer deadline count reversed = do
      next <- receiveSomeBytesUntilOn slot epoch buffer (min 64 (total - count)) deadline
      case next of
        Left err -> pure (Left err)
        Right bytes -> go buffer deadline (count + B.length bytes) (bytes : reversed)
-- | TLS application records can sit behind an SMP long-poll for minutes.
-- Keep the socket alive across empty polls and preserve every partial read
-- until the TLS backend has received the exact requested length.
receiveExactlyBytesForever :: Int -> IO (Either NetworkError B.ByteString)
receiveExactlyBytesForever total = receiveExactlyBytesForeverOn 0 0 total

receiveExactlyBytesForeverOn :: Word8 -> Word8 -> Int -> IO (Either NetworkError B.ByteString)
receiveExactlyBytesForeverOn slot epoch total
  | total < 0 = pure (Left InvalidInput)
  | otherwise = allocaBytes 64 $ \buffer -> go buffer 0 []
  where
    go _ count chunks | count == total = pure (Right (B.concat (reverse chunks)))
    go buffer count chunks = do
      let capacity = min 64 (total - count)
      alive <- guardEpoch slot epoch
      case alive of
        Left err -> pure (Left err)
        Right () -> do
          result <- locked $ tcpRecv buffer (fromIntegral capacity) slot
          case result of
            n | n > 0 && n <= fromIntegral capacity -> do
              bytes <- B.packCStringLen (castPtr buffer, fromIntegral n)
              go buffer (count + B.length bytes) (bytes : chunks)
            0 -> threadDelay 10000 >> go buffer count chunks
            -2 -> pure (Left Closed)
            _ -> pure (Left TransportError)

closeSession :: Int -> IO (Either NetworkError ())
closeSession = closeSessionOn 0 0

-- | Close only if the caller still owns this slot generation.
closeSessionOn :: Word8 -> Word8 -> Int -> IO (Either NetworkError ())
closeSessionOn slot epoch timeoutMs = do
  deadline <- deadlineAfter timeoutMs
  retryPending deadline $ locked $ do
    alive <- guardEpoch slot epoch
    case alive of
      Left err -> pure (Left err)
      Right () -> do
        result <- tcpClose slot
        case result of
          1 -> bumpEpoch slot >> pure (Right ())
          0 -> pure (Left TimedOut)
          _ -> pure (Left TransportError)

cancelSession :: IO ()
cancelSession = cancelSessionOn 0

-- | Unconditional abort used when reclaiming the ephemeral slot for XFTP.
cancelSessionOn :: Word8 -> IO ()
cancelSessionOn slot = locked $ do
  tcpCancel slot
  _ <- bumpEpoch slot
  pure ()

-- | Abort only if this backend still owns the slot generation.
cancelSessionEpoch :: Word8 -> Word8 -> IO ()
cancelSessionEpoch slot epoch = locked $ do
  alive <- guardEpoch slot epoch
  case alive of
    Left _ -> pure ()
    Right () -> do
      tcpCancel slot
      _ <- bumpEpoch slot
      pure ()
