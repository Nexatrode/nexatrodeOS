{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Data.Bits (shiftR)
import Data.Word (Word8, Word16, Word32)
import SimplexIPC

main :: IO ()
main = do
  let requestBytes = encodeRequest 4 41 "conn-1\0hello"
      expected = Request SendMessage 41 "conn-1\0hello"
  assert "decode Rust-compatible SendMessage request" (decodeRequest requestBytes == Right expected)
  case decodeRequest requestBytes of
    Left err -> fail err
    Right request -> do
      response <- either fail pure (encodeResponse request Ok "sent")
      assert "fixed-size shared page" (B.length response == pageSize)
      assert "correlated response" (decodeResponse 41 response == Right (Ok, "sent"))
      assert "stale response rejected" (isLeft (decodeResponse 40 response))

  assert "unknown operation rejected" (isLeft (decodeRequest (encodeRequest 99 1 "")))
  assert "zero request ID rejected" (isLeft (decodeRequest (encodeRequest 1 0 "")))
  assert "oversized request rejected"
    (isLeft (decodeRequest (encodeRequest 1 1 (B.replicate (payloadCapacity + 1) 0))))
  assert "truncated payload rejected"
    (isLeft (decodeRequest (B.take (headerSizeForTest + 2) (encodeRequest 4 2 "abc"))))
  payload <- either fail pure (encodePayload [F8 2, F64 0x0123456789abcdef, FBytes "hello"])
  assert "Rust-compatible operation payload vector"
    (payload == B.pack [2, 0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01, 5, 0, 104, 101, 108, 108, 111])
  assert "typed payload decodes with strict schema"
    (decodePayload [1, 3, 4] payload == Right [F8 2, F64 0x0123456789abcdef, FBytes "hello"])
  assert "truncated typed payload rejected" (isLeft (decodePayload [1, 3, 4] (B.init payload)))
  assert "trailing typed payload rejected" (isLeft (decodePayload [1, 3, 4] (payload <> "x")))
  assert "oversized typed payload rejected before encoding"
    (isLeft (encodePayload [FBytes (B.replicate payloadCapacity 0)]))
  putStrLn "Simplex IPC ABI tests: PASS"

assert :: String -> Bool -> IO ()
assert label condition = unless condition (fail ("FAIL: " <> label))

isLeft :: Either a b -> Bool
isLeft (Left _) = True
isLeft _ = False

encodeRequest :: Word16 -> Word32 -> B.ByteString -> B.ByteString
encodeRequest operation requestId payload =
  B.pack ([78, 88, 83, 77] <> le16 1 <> le16 operation <> le32 requestId <> le32 0
    <> le32 (fromIntegral (B.length payload)))
    <> payload
    <> B.replicate (payloadCapacity - B.length payload) 0

headerSizeForTest :: Int
headerSizeForTest = 20

le16 :: Word16 -> [Word8]
le16 value = [fromIntegral value, fromIntegral (value `div` 256)]

le32 :: Word32 -> [Word8]
le32 value = [fromIntegral (value `shiftR` n) | n <- [0, 8, 16, 24]]
