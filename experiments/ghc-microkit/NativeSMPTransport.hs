-- | Fixed-block SMP framing over a TLS context supplied by the guest's
-- protected network capability. This is the platform seam below the future
-- upstream client; it does not implement SimpleX agent messages.
module NativeSMPTransport
  ( readFrame,
    sendFrame,
    sendBatch,
    decodeBatch,
  ) where

import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.Word (Word16)
import qualified Network.TLS as TLS
import qualified Simplex.Messaging.Encoding as SE
import qualified Simplex.Messaging.Transport.Buffer as TB

blockSize :: Int
blockSize = 16384

readFrame :: TB.TBuffer -> TLS.Context -> IO B.ByteString
readFrame buffer ctx = do
  block <- TB.getBuffered buffer blockSize (Just 5000000) (TLS.recvData ctx)
  if B.length block /= blockSize
    then ioError (userError "bad SMP block size")
    else case SE.smpDecode (B.take 2 block) :: Either String Word16 of
      Left err -> ioError (userError ("bad SMP block length: " ++ err))
      Right size -> do
        let n = fromIntegral size
            payload = B.take n (B.drop 2 block)
            padding = B.drop (n + 2) block
        if n > blockSize - 2 || B.length payload /= n || B.any (/= 35) padding
          then ioError (userError "bad SMP block payload or padding")
          else pure payload

sendFrame :: TLS.Context -> B.ByteString -> IO ()
sendFrame ctx payload
  | B.length payload > blockSize - 2 = ioError (userError "SMP frame too large")
  | otherwise = TLS.sendData ctx . L.fromStrict $ B.concat
      [SE.smpEncode (fromIntegral (B.length payload) :: Word16),
       payload, B.replicate (blockSize - 2 - B.length payload) 35]

sendBatch :: TLS.Context -> B.ByteString -> IO ()
sendBatch ctx transmission
  | B.length transmission > blockSize - 5 =
      ioError (userError "SMP transmission too large")
  | otherwise = sendFrame ctx $ B.concat
      [B.singleton 1,
       SE.smpEncode (fromIntegral (B.length transmission) :: Word16),
       transmission]

decodeBatch :: B.ByteString -> Maybe B.ByteString
decodeBatch payload
  | B.length payload < 3 || B.head payload /= 1 = Nothing
  | otherwise = case SE.smpDecode (B.take 2 (B.drop 1 payload)) :: Either String Word16 of
      Right size | fromIntegral size == B.length payload - 3 ->
        Just (B.drop 3 payload)
      _ -> Nothing
