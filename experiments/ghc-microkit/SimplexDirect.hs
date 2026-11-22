{-# LANGUAGE OverloadedStrings #-}
module SimplexDirect (DirectChange(..), parseChange, frameDirect, splitChatBatch) where

import qualified Data.Aeson as A
import Data.Aeson ((.:))
import Data.Aeson.Types (Parser)
import qualified Data.ByteString.Lazy as BL
import qualified Data.ByteString as B
import Foreign.C.Types (CSize(..))
import Foreign.Marshal.Alloc (allocaBytes)
import Foreign.Ptr (Ptr, castPtr)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Data.Word (Word8, Word16, Word64)
import Data.Bits (shiftR)

data DirectChange = Update T.Text T.Text | Delete T.Text deriving (Eq, Show)

parseChange :: T.Text -> A.Object -> Parser DirectChange
parseChange "x.msg.update" params = do
  target <- params .: "msgId"
  content <- params .: "content"
  A.withObject "updated content" (\o -> do
    kind <- o .: "type"
    if kind == ("text" :: T.Text) then Update target <$> o .: "text"
      else fail "unsupported updated content") content
parseChange "x.msg.del" params = Delete <$> params .: "msgId"
parseChange _ _ = fail "unsupported SimpleX event"

frameDirect :: Word8 -> Word64 -> Word64 -> Word16 -> T.Text -> B.ByteString -> B.ByteString
frameDirect kind handle message part shared body =
  B.concat [ B.pack ([kind] <> le 8 handle <> le 8 message <> le 2 part <> le 2 (B.length sid)),
             sid, B.pack (le 4 (B.length body) <> le 2 (B.length first)), first ]
  where
    sid = TE.encodeUtf8 shared
    first = B.take 3000 body
    le :: Integral a => Int -> a -> [Word8]
    le size value = [fromIntegral ((fromIntegral value :: Word64) `shiftR` (8 * i)) | i <- [0 .. size - 1]]

-- SimpleX PQ contacts wrap large messages in 'X', a counted list of
-- passthrough (tag '0', u8 length) or Zstandard (tag '1', u16 BE) items.
-- Both per-frame and aggregate output obey upstream maxDecompressedMsgLength.
foreign import ccall unsafe "ZSTD_getFrameContentSize"
  frameSize :: Ptr Word8 -> CSize -> IO Word64
foreign import ccall unsafe "ZSTD_decompress"
  decompress :: Ptr Word8 -> CSize -> Ptr Word8 -> CSize -> IO CSize

splitChatBatch :: B.ByteString -> IO (Either String [B.ByteString])
splitChatBatch input = case B.uncons input of
  Just (88, compressed) -> case B.uncons compressed of
    Just (count, rest) | count > 0 -> unpackItems count 65536 rest
    _ -> pure (Left "empty compressed chat batch")
  _ -> pure (plain input)
  where
    plain body = case B.uncons body of
      Just (91, _) -> map (BL.toStrict . A.encode) <$> (A.eitherDecodeStrict' body :: Either String [A.Value])
      Just (61, batch) -> case B.uncons batch of
        Just (count, rest) -> binaryItems count rest
        Nothing -> Left "empty binary chat batch"
      _ -> Right [body]

    binaryItems 0 rest
      | B.null rest = Right []
      | otherwise = Left "trailing binary chat data"
    binaryItems n bs = do
      (item, rest) <- takeItem 2 bs
      (item :) <$> binaryItems (n - 1) rest

    unpackItems :: Word8 -> Int -> B.ByteString -> IO (Either String [B.ByteString])
    unpackItems 0 _ rest = pure $ if B.null rest then Right [] else Left "trailing compressed chat data"
    unpackItems count budget bytes = case B.uncons bytes of
      Just (tag, fields) | tag == 48 || tag == 49 ->
        case takeItem (if tag == 48 then 1 else 2) fields of
          Left err -> pure (Left err)
          Right (item, rest) -> do
            unpacked <- if tag == 49 then decompressBounded budget item else pure (Right item)
            case unpacked of
              Right body | B.length body <= budget -> case plain body of
                Right events -> fmap (events <>) <$> unpackItems (count - 1) (budget - B.length body) rest
                Left err -> pure (Left err)
              _ -> pure (Left "invalid or oversized compressed chat data")
      _ -> pure (Left "invalid compressed chat item")

    takeItem width fields =
      let (prefix, body) = B.splitAt width fields
          len = B.foldl' (\n b -> n * 256 + fromIntegral b) 0 prefix
       in if B.length prefix == width && len <= B.length body
            then Right (B.splitAt len body) else Left "invalid chat item length"

decompressBounded :: Int -> B.ByteString -> IO (Either String B.ByteString)
decompressBounded budget bytes = B.useAsCStringLen bytes $ \(src, n) -> do
  size <- frameSize (castPtr src) (fromIntegral n)
  -- Missing size and error sentinels are both larger than the budget.
  if size == 0 || size > fromIntegral budget then pure (Left "invalid zstd frame size")
    else allocaBytes (fromIntegral size) $ \dst -> do
      actual <- decompress dst (fromIntegral size) (castPtr src) (fromIntegral n)
      if actual /= fromIntegral size then pure (Left "invalid zstd frame")
        else Right <$> B.packCStringLen (castPtr dst, fromIntegral size)
