{-# LANGUAGE OverloadedStrings #-}
import qualified Codec.Compression.Zstd as Zstd
import qualified Data.Aeson as A
import Data.Aeson ((.:))
import Data.Aeson.Types (parseEither)
import qualified Data.ByteString as B
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import Control.Monad (unless)
import SimplexDirect

change :: B.ByteString -> Either String DirectChange
change bytes = do
  event <- A.eitherDecodeStrict' bytes
  parseEither (A.withObject "event" $ \o -> do
    name <- o .: "event"
    params <- o .: "params"
    parseChange name params) event

main :: IO ()
main = do
  let check condition = unless condition (fail "direct message fixture failed")
  check (change "{\"event\":\"x.msg.update\",\"msgId\":\"envelope\",\"params\":{\"msgId\":\"target\",\"content\":{\"type\":\"text\",\"text\":\"changed\"}}}" == Right (Update "target" "changed"))
  check (change "{\"event\":\"x.msg.del\",\"params\":{\"msgId\":\"target\"}}" == Right (Delete "target"))
  let body = TE.encodeUtf8 (T.replicate 1100 "🍎")
      frame = frameDirect 7 12 49 2 "target" body
  check (B.take 19 frame == B.pack ([7,12] ++ replicate 7 0 ++ [49] ++ replicate 7 0 ++ [2,0]))
  check (B.take 14 (B.drop 19 frame) == B.pack ([6,0] ++ B.unpack "target" ++ [48,17,0,0,184,11]))
  check (B.drop 33 frame == B.take 3000 body)
  let json = B.concat ["{\"event\":\"x.msg.new\",\"params\":{\"content\":{\"type\":\"text\",\"text\":\"", B.replicate 14000 97, "\"}}}"]
      pack body' = let z = Zstd.compress 3 body'; n = B.length z
                    in B.pack [88,1,49,fromIntegral (n `div` 256),fromIntegral n] <> z
  unpacked <- splitChatBatch (pack json)
  check (unpacked == Right [json])
  oversized <- splitChatBatch (pack (B.replicate 65537 97))
  check (either (const True) (const False) oversized)
