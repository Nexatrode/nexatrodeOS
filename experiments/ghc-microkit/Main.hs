module Main where

import Control.Concurrent (threadDelay)
import qualified Data.ByteString as B
import Data.Word (Word8)
import Foreign.C.Types (CInt(..))
import Foreign.Marshal.Alloc (allocaBytes)
import Foreign.Marshal.Array (peekArray)
import Foreign.Ptr (Ptr)
import GHC.Clock (getMonotonicTimeNSec)
import NativeNetwork

foreign import ccall unsafe "sel4_haskell_entropy16"
    entropy16 :: Ptr Word8 -> IO CInt

tcpEchoCheck :: IO ()
tcpEchoCheck = do
    resolved <- resolveIPv4 "personal-os.test" 3000
    case resolved of
      Left Unavailable -> putStrLn "HASKELL_TCP|SKIP"
      _ -> do
        putStrLn (if resolved == Right (IPv4 10 0 2 2)
                  then "HASKELL_DNS|PASS" else "HASKELL_DNS|FAIL")
        case resolved of
          Right address | address == IPv4 10 0 2 2 -> runSessions address
          _ -> cancelSession >> putStrLn "HASKELL_TCP|FAIL"
  where
    streamPayload = B.take 192 (B.concat (replicate 14 (ascii "NATIVE-STREAM-")))
    reconnectPayload = ascii "RECONNECT-OK"
    stallPayload = ascii "STALL-THEN-CANCEL"
    recoverPayload = ascii "RECOVER-OK"
    -- Larger than upstream SimpleXMQ's 16 KiB SMP transport block.
    burstPayload = B.take 20480 (B.concat (replicate 1500 (ascii "INBOUND-BURST-")))
    outboundPayload = B.take 20480 (B.concat (replicate 1500 (ascii "OUTBOUND-BURST-")))
    outboundAck = ascii "OUTBOUND-OK"
    ascii = B.pack . map (fromIntegral . fromEnum) :: String -> B.ByteString
    open address = (== Right ()) <$> connectIPv4 address 1241 3000
    close = (== Right ()) <$> closeSession 3000
    exchange payload = do
      sent <- sendBytes payload 5000
      if sent /= Right () then pure False else do
        received <- receiveExactlyBytes (B.length payload) 5000
        pure (received == Right payload)

    runSessions address = do
      firstOpen <- open address
      first <- if firstOpen then exchange streamPayload else pure False
      firstClosed <- if first then close else pure False
      putStrLn (if first && firstClosed
                then "HASKELL_TCP|STREAM_PASS" else "HASKELL_TCP|STREAM_FAIL")
      secondOpen <- if first && firstClosed then open address else pure False
      second <- if secondOpen then exchange reconnectPayload else pure False
      secondClosed <- if second then close else pure False
      putStrLn (if second && secondClosed
                then "HASKELL_TCP|RECONNECT_PASS" else "HASKELL_TCP|RECONNECT_FAIL")
      thirdOpen <- if second && secondClosed then open address else pure False
      stalled <- if thirdOpen then (== Right ()) <$> sendBytes stallPayload 5000
                 else pure False
      timedOut <- if stalled then (== Left TimedOut) <$> receiveSomeBytes 64 250
                  else pure False
      cancelSession
      putStrLn (if timedOut then "HASKELL_TCP|TIMEOUT_PASS"
                else "HASKELL_TCP|TIMEOUT_FAIL")
      fourthOpen <- if timedOut then open address else pure False
      fourth <- if fourthOpen then exchange recoverPayload else pure False
      fourthClosed <- if fourth then close else pure False
      putStrLn (if fourth && fourthClosed
                then "HASKELL_TCP|RECOVER_PASS" else "HASKELL_TCP|RECOVER_FAIL")
      fifthOpen <- if fourth && fourthClosed then open address else pure False
      burst <- if fifthOpen then (== Right burstPayload) <$>
                 receiveExactlyBytes (B.length burstPayload) 5000 else pure False
      fifthClosed <- if burst then close else pure False
      putStrLn (if burst && fifthClosed
                then "HASKELL_TCP|BURST_PASS" else "HASKELL_TCP|BURST_FAIL")
      sixthOpen <- if burst && fifthClosed then open address else pure False
      outbound <- if sixthOpen then do
          sent <- sendBytes outboundPayload 5000
          if sent /= Right () then pure False else do
            ack <- receiveExactlyBytes (B.length outboundAck) 5000
            pure (ack == Right outboundAck)
        else pure False
      sixthClosed <- if outbound then close else pure False
      putStrLn (if outbound && sixthClosed
                then "HASKELL_TCP|OUTBOUND_PASS" else "HASKELL_TCP|OUTBOUND_FAIL")
      if first && firstClosed && second && secondClosed &&
         timedOut && fourth && fourthClosed && burst && fifthClosed &&
         outbound && sixthClosed
        then putStrLn "HASKELL_TCP|PASS"
        else cancelSession >> putStrLn "HASKELL_TCP|FAIL"

getEntropy16 :: IO (Maybe [Word8])
getEntropy16 = allocaBytes 16 $ \buffer -> do
    ok <- entropy16 buffer
    if ok == 1 then Just <$> peekArray 16 buffer else pure Nothing

main :: IO ()
main = do
    putStrLn "hello from Haskell"
    start <- getMonotonicTimeNSec
    threadDelay 200000
    end <- getMonotonicTimeNSec
    putStrLn ("HASKELL_DELAY_NS|" ++ show (end - start))
    first <- getEntropy16
    second <- getEntropy16
    if case (first, second) of
        (Just a, Just b) -> any (/= 0) a && any (/= 0) b && a /= b
        _ -> False
      then putStrLn "HASKELL_ENTROPY|PASS"
      else putStrLn "HASKELL_ENTROPY|FAIL"
    tcpEchoCheck
