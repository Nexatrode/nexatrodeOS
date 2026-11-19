-- | Byte-level counterpart of rust/src/simplex_ipc.rs. This module contains no
-- host sockets or file access; the Agent PD uses it over its shared page.
module SimplexIPC
  ( Operation (..),
    Status (..),
    Request (..),
    Field (..),
    decodeRequest,
    encodeResponse,
    decodeResponse,
    encodePayload,
    decodePayload,
    pageSize,
    payloadCapacity,
  )
where

import Data.Bits ((.|.), shiftL, shiftR)
import qualified Data.ByteString as B
import Data.Word (Word16, Word32, Word64, Word8)

pageSize :: Int
pageSize = 4096

headerSize :: Int
headerSize = 20

payloadCapacity :: Int
payloadCapacity = pageSize - headerSize

data Operation
  = ListConnections
  | CreateInvitation
  | JoinInvitation
  | SendMessage
  | ReceiveNext
  | Acknowledge
  | DeleteConnection
  deriving (Eq, Show)

data Status = RequestPending | Ok | NoEvent | Invalid | Unavailable | Failed
  deriving (Eq, Show)

data Request = Request
  { operation :: Operation,
    requestId :: Word32,
    requestPayload :: B.ByteString
  }
  deriving (Eq, Show)

-- | Operation payload fields have no tags: each operation has a fixed schema
-- documented in docs/simplex-port-feasibility.md. Byte strings carry a u16 LE
-- byte length; integer fields use their natural little-endian width.
data Field = F8 Word8 | F16 Word16 | F64 Word64 | FBytes B.ByteString
  deriving (Eq, Show)

encodePayload :: [Field] -> Either String B.ByteString
encodePayload fields = do
  if any oversizedByteString fields
    then Left "SimpleX byte-string field too large"
    else pure ()
  let encodedLength = sum (map fieldSize fields)
  if encodedLength > payloadCapacity
    then Left "SimpleX operation payload too large"
    else Right (B.concat (map encodeField fields))

fieldSize :: Field -> Int
fieldSize field = case field of
  F8 _ -> 1
  F16 _ -> 2
  F64 _ -> 8
  FBytes value -> 2 + B.length value

oversizedByteString :: Field -> Bool
oversizedByteString field = case field of
  FBytes value -> B.length value > fromIntegral (maxBound :: Word16)
  _ -> False

decodePayload :: [Word8] -> B.ByteString -> Either String [Field]
decodePayload schema payload = go schema payload []
  where
    go [] rest acc
      | B.null rest = Right (reverse acc)
      | otherwise = Left "trailing SimpleX operation payload bytes"
    go (kind : kinds) rest acc = case kind of
      1 -> do
        (value, remaining) <- takeFixed 1 rest
        go kinds remaining (F8 (B.index value 0) : acc)
      2 -> do
        (value, remaining) <- takeFixed 2 rest
        go kinds remaining (F16 (read16 value 0) : acc)
      3 -> do
        (value, remaining) <- takeFixed 8 rest
        go kinds remaining (F64 (read64 value 0) : acc)
      4 -> do
        if B.length rest < 2 then Left "truncated byte-string length" else pure ()
        let fieldLength = fromIntegral (read16 rest 0)
            body = B.drop 2 rest
        if B.length body < fieldLength
          then Left "truncated byte-string payload"
          else go kinds (B.drop fieldLength body) (FBytes (B.take fieldLength body) : acc)
      _ -> Left "unknown SimpleX payload schema field"

encodeField :: Field -> B.ByteString
encodeField field = B.pack $ case field of
  F8 value -> [value]
  F16 value -> le16 value
  F64 value -> le64 value
  FBytes value -> le16 (fromIntegral (B.length value)) <> B.unpack value

takeFixed :: Int -> B.ByteString -> Either String (B.ByteString, B.ByteString)
takeFixed count bytes
  | B.length bytes < count = Left "truncated fixed-width payload field"
  | otherwise = Right (B.take count bytes, B.drop count bytes)

decodeRequest :: B.ByteString -> Either String Request
decodeRequest page
  | B.length page < headerSize = Left "short SimpleX IPC header"
  | B.take 4 page /= B.pack [78, 88, 83, 77] = Left "bad SimpleX IPC magic"
  | read16 page 4 /= 1 = Left "unsupported SimpleX IPC version"
  | read32 page 12 /= 0 = Left "page is not a pending request"
  | ident == 0 = Left "zero SimpleX IPC request ID"
  | len > payloadCapacity = Left "SimpleX IPC request payload too large"
  | B.length page < headerSize + len = Left "truncated SimpleX IPC payload"
  | otherwise = case decodeOperation (read16 page 6) of
      Nothing -> Left "unknown SimpleX IPC operation"
      Just op -> Right (Request op ident (B.take len (B.drop headerSize page)))
  where
    ident = read32 page 8
    len = fromIntegral (read32 page 16)

encodeResponse :: Request -> Status -> B.ByteString -> Either String B.ByteString
encodeResponse request status payload
  | status == RequestPending = Left "cannot publish a pending request as response"
  | B.length payload > payloadCapacity = Left "SimpleX IPC response payload too large"
  | otherwise =
      Right $ B.pack (magic <> le16 1 <> le16 (encodeOperation (operation request))
        <> le32 (requestId request) <> le32 (encodeStatus status)
        <> le32 (fromIntegral (B.length payload))) <> payload
        <> B.replicate (payloadCapacity - B.length payload) 0

decodeResponse :: Word32 -> B.ByteString -> Either String (Status, B.ByteString)
decodeResponse expected page
  | B.length page < headerSize = Left "short SimpleX IPC response header"
  | B.take 4 page /= B.pack magic = Left "bad SimpleX IPC response magic"
  | read16 page 4 /= 1 = Left "unsupported SimpleX IPC response version"
  | expected == 0 || read32 page 8 /= expected = Left "stale SimpleX IPC response"
  | len > payloadCapacity = Left "SimpleX IPC response payload too large"
  | B.length page < headerSize + len = Left "truncated SimpleX IPC response payload"
  | otherwise = case decodeStatus (read32 page 12) of
      Nothing -> Left "invalid SimpleX IPC response status"
      Just status -> Right (status, B.take len (B.drop headerSize page))
  where
    len = fromIntegral (read32 page 16)

magic :: [Word8]
magic = [78, 88, 83, 77]

decodeOperation :: Word16 -> Maybe Operation
decodeOperation value = case value of
  1 -> Just ListConnections
  2 -> Just CreateInvitation
  3 -> Just JoinInvitation
  4 -> Just SendMessage
  5 -> Just ReceiveNext
  6 -> Just Acknowledge
  7 -> Just DeleteConnection
  _ -> Nothing

encodeOperation :: Operation -> Word16
encodeOperation op = case op of
  ListConnections -> 1
  CreateInvitation -> 2
  JoinInvitation -> 3
  SendMessage -> 4
  ReceiveNext -> 5
  Acknowledge -> 6
  DeleteConnection -> 7

encodeStatus :: Status -> Word32
encodeStatus status = case status of
  RequestPending -> 0
  Ok -> 1
  NoEvent -> 2
  Invalid -> 3
  Unavailable -> 4
  Failed -> 5

decodeStatus :: Word32 -> Maybe Status
decodeStatus value = case value of
  1 -> Just Ok
  2 -> Just NoEvent
  3 -> Just Invalid
  4 -> Just Unavailable
  5 -> Just Failed
  _ -> Nothing

read16 :: B.ByteString -> Int -> Word16
read16 bytes offset = fromIntegral (bytes `at` offset) .|. shiftL (fromIntegral (bytes `at` (offset + 1))) 8

read32 :: B.ByteString -> Int -> Word32
read32 bytes offset =
  fromIntegral (bytes `at` offset)
    .|. shiftL (fromIntegral (bytes `at` (offset + 1))) 8
    .|. shiftL (fromIntegral (bytes `at` (offset + 2))) 16
    .|. shiftL (fromIntegral (bytes `at` (offset + 3))) 24

at :: B.ByteString -> Int -> Word8
at = B.index

le16 :: Word16 -> [Word8]
le16 value = [fromIntegral value, fromIntegral (value `shiftR` 8)]

le32 :: Word32 -> [Word8]
le32 value = [fromIntegral (value `shiftR` n) | n <- [0, 8, 16, 24]]

read64 :: B.ByteString -> Int -> Word64
read64 bytes offset = foldr (.|.) 0
  [shiftL (fromIntegral (at bytes (offset + n `div` 8))) n | n <- [0, 8, 16, 24, 32, 40, 48, 56]]

le64 :: Word64 -> [Word8]
le64 value = [fromIntegral (value `shiftR` n) | n <- [0, 8, 16, 24, 32, 40, 48, 56]]
