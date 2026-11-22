-- | QEMU-only wire-level PING/PONG and signed queue creation with the
-- upstream SMP protocol server. It does not compile upstream's client or send
-- an end-to-end encrypted message.
module Main where

import Control.Exception (SomeException, try)
import Data.List (isInfixOf)
import Crypto.Error (CryptoFailable(..))
import qualified Crypto.PubKey.Curve25519 as X25519
import qualified Crypto.PubKey.Ed25519 as Ed25519
import Crypto.Random.Entropy (getEntropy)
import qualified Data.ByteArray as BA
import qualified Data.ByteString as B
import Data.X509 (decodeSignedCertificate)
import qualified Data.X509 as X
import qualified Data.X509.Validation as XV
import NativeNetwork (resolveIPv4)
import NativeSMPConnection (withPinnedSMPConnection)
import qualified NativeSMPTransport as ST
import qualified Network.TLS as TLS
import SMPFixtureCA (caDER)
import qualified Simplex.Messaging.Encoding as SE
import qualified Simplex.Messaging.Transport.Buffer as TB

main :: IO ()
main = case decodeSignedCertificate caDER of
  Left err -> putStrLn ("SMP_CA|FAIL|" ++ err)
  Right ca -> do
    let XV.Fingerprint pin = XV.getFingerprint ca X.HashSHA256
    resolved <- resolveIPv4 "personal-os.test" 5000
    case resolved of
      Left err -> putStrLn ("SMP_DNS|FAIL|" ++ show err)
      Right _address -> do
        putStrLn "SMP_DNS|PASS"
        runProbe pin

runProbe :: B.ByteString -> IO ()
runProbe pin = do
  result <- try $ withPinnedSMPConnection "personal-os.test" "65224" pin 5000 $ \ctx -> do
    putStrLn "SMP_CONNECT|PASS"
    putStrLn "SMP_TLS_PIN|PASS"
    buffer <- TB.newTBuffer
    block <- ST.readFrame buffer ctx
    inspectBlock buffer ctx pin block
  case (result :: Either SomeException ()) of
    Left err -> putStrLn ("SMP_PROBE|FAIL|" ++ show err)
    Right () -> rejectWrongPin

rejectWrongPin :: IO ()
rejectWrongPin = do
  result <- try $ withPinnedSMPConnection "personal-os.test" "65224"
    (B.replicate 32 0) 5000 (\_ -> pure ())
  case (result :: Either SomeException ()) of
    Left err | "UnknownCa" `isInfixOf` show err ->
      putStrLn "SMP_WRONG_PIN|REJECT"
    Left err -> putStrLn ("SMP_WRONG_PIN|OTHER_FAIL|" ++ show err)
    Right () -> putStrLn "SMP_WRONG_PIN|ACCEPTED"

inspectBlock :: TB.TBuffer -> TLS.Context -> B.ByteString -> B.ByteString -> IO ()
inspectBlock buffer ctx pin body = do
  if B.length body < 5
    then putStrLn "SMP_BLOCK|FAIL"
    else do
      putStrLn "SMP_UPSTREAM_ENCODING|PASS"
      putStrLn ("SMP_BLOCK|PASS|" ++ show (B.length body))
      let minVersion = fromIntegral (B.index body 0) * 256 +
                       fromIntegral (B.index body 1) :: Int
          maxVersion = fromIntegral (B.index body 2) * 256 +
                       fromIntegral (B.index body 3) :: Int
      if minVersion <= 6 && maxVersion >= 6
        then do
          putStrLn ("SMP_VERSION_RANGE|" ++ show minVersion ++ "|" ++ show maxVersion)
          let sessionSize = fromIntegral (B.index body 4) :: Int
              session = B.take sessionSize (B.drop 5 body)
          finished <- TLS.getFinished ctx
          if sessionSize == 32 && finished == Just session
            then do
              putStrLn "SMP_SESSION|PASS"
              -- Upstream v6 encodes (VersionSMP 6, KeyHash pin), then pads
              -- the payload to a fixed 16 KiB block with '#'.
              let response = B.pack [0, 6, 32] `B.append` pin
              ST.sendFrame ctx response
              putStrLn "SMP_CLIENT_HANDSHAKE|SENT"
              let transmission = B.concat
                    [ B.singleton 0              -- no authenticator
                    , B.cons 32 session          -- TLS-unique session ID
                    , B.pack [1, 112, 0]         -- correlation "p", no entity
                    , B.pack [80, 73, 78, 71]    -- PING
                    ]
              ST.sendBatch ctx transmission
              putStrLn "SMP_PING|SENT"
              pong <- ST.readFrame buffer ctx
              inspectPong session pong
              createQueue buffer ctx session
            else putStrLn "SMP_SESSION|FAIL"
        else putStrLn "SMP_VERSION_RANGE|FAIL"

inspectPong :: B.ByteString -> B.ByteString -> IO ()
inspectPong session payload = do
  let expected = B.concat [B.pack [0, 32], session,
                           B.pack [1, 112, 0, 80, 79, 78, 71]]
  if ST.decodeBatch payload == Just expected
    then putStrLn "SMP_PONG|PASS"
    else putStrLn "SMP_PONG|FAIL"

-- A fixture-only queue. Keys are generated inside the guest and deliberately
-- never logged. The two DER prefixes are the RFC 8410 SubjectPublicKeyInfo
-- encodings used by SimpleXMQ's Crypto.encodePubKey.
createQueue :: TB.TBuffer -> TLS.Context -> B.ByteString -> IO ()
createQueue buffer ctx session = do
  authSeed <- getEntropy 32 :: IO B.ByteString
  dhSeed <- getEntropy 32 :: IO B.ByteString
  case (Ed25519.secretKey authSeed, X25519.secretKey dhSeed) of
    (CryptoPassed authSecret, CryptoPassed dhSecret) -> do
      let authPublic = Ed25519.toPublic authSecret
          dhPublic = X25519.toPublic dhSecret
          edDER = B.pack [0x30,0x2a,0x30,0x05,0x06,0x03,0x2b,0x65,
                          0x70,0x03,0x21,0x00] `B.append` BA.convert authPublic
          xDER = B.pack [0x30,0x2a,0x30,0x05,0x06,0x03,0x2b,0x65,
                         0x6e,0x03,0x21,0x00] `B.append` BA.convert dhPublic
          command = B.concat [B.pack [78,69,87,32], smpBytes edDER,
                              smpBytes xDER, B.singleton 67]
          signedTransmission = B.concat
            [smpBytes session, B.pack [1, 113, 0], command]
          signature = Ed25519.sign authSecret authPublic signedTransmission
          transmission = smpBytes (BA.convert signature) `B.append`
                         signedTransmission
      ST.sendBatch ctx transmission
      putStrLn "SMP_NEW|SENT"
      reply <- ST.readFrame buffer ctx
      ids <- inspectIds session reply
      case ids of
        Just (recipientId, senderId) -> do
          sent <- sendOpaque buffer ctx session senderId
          if sent then receiveOpaque buffer ctx session recipientId authSecret authPublic
                  else pure ()
        Nothing -> pure ()
    _ -> putStrLn "SMP_NEW|KEY_FAIL"

smpBytes :: B.ByteString -> B.ByteString
smpBytes = SE.smpEncode

inspectIds :: B.ByteString -> B.ByteString -> IO (Maybe (B.ByteString, B.ByteString))
inspectIds session payload = case ST.decodeBatch payload of
  Nothing -> putStrLn "SMP_IDS|FAIL|BLOCK" >> pure Nothing
  Just transmission -> do
    let prefix = B.concat [B.singleton 0, smpBytes session,
                           B.pack [1, 113, 0, 73, 68, 83, 32]]
        fields = B.drop (B.length prefix) transmission
    if prefix `B.isPrefixOf` transmission && validIdsFields fields
      then do
        putStrLn "SMP_IDS|PASS"
        case takeField fields of
          Just (recipient, remaining) -> case takeField remaining of
            Just (sender, _) -> pure (Just (recipient, sender))
            _ -> pure Nothing
          _ -> pure Nothing
      else putStrLn "SMP_IDS|FAIL|RESPONSE" >> pure Nothing

-- The SMP relay treats the body as opaque. This is a transport-only payload,
-- not an encrypted SimpleX agent message.
sendOpaque :: TB.TBuffer -> TLS.Context -> B.ByteString -> B.ByteString -> IO Bool
sendOpaque buffer ctx session senderId = do
  let command = B.pack [83,69,78,68,32,70,32,81,69,77,85,45,83,77,80]
      transmission = B.concat [B.singleton 0, smpBytes session,
                               B.singleton 1, B.singleton 114,
                               smpBytes senderId, command]
  ST.sendBatch ctx transmission
  putStrLn "SMP_SEND|SENT"
  reply <- ST.readFrame buffer ctx
  let expected = B.concat [B.singleton 0, smpBytes session,
                           B.singleton 1, B.singleton 114,
                           smpBytes senderId, B.pack [79,75]]
  if ST.decodeBatch reply == Just expected
    then putStrLn "SMP_SEND_OK|PASS" >> pure True
    else putStrLn "SMP_SEND_OK|FAIL" >> pure False

receiveOpaque :: TB.TBuffer -> TLS.Context -> B.ByteString -> B.ByteString ->
                 Ed25519.SecretKey -> Ed25519.PublicKey -> IO ()
receiveOpaque buffer ctx session recipientId authSecret authPublic = do
  let signedTransmission = B.concat
        [smpBytes session, B.singleton 1, B.singleton 115,
         smpBytes recipientId, B.pack [71,69,84]]
      signature = Ed25519.sign authSecret authPublic signedTransmission
      transmission = smpBytes (BA.convert signature) `B.append`
                     signedTransmission
  ST.sendBatch ctx transmission
  putStrLn "SMP_GET|SENT"
  reply <- ST.readFrame buffer ctx
  let prefix = B.concat [B.singleton 0, smpBytes session,
                         B.singleton 1, B.singleton 115,
                         smpBytes recipientId, B.pack [77,83,71,32]]
  if maybe False (\response ->
       let suffix = B.drop (B.length prefix) response in
       prefix `B.isPrefixOf` response &&
       maybe False (\(msgId, body) -> not (B.null msgId) &&
                   B.length body > 16) (takeField suffix)) (ST.decodeBatch reply)
    then putStrLn "SMP_MSG|PASS"
    else putStrLn "SMP_MSG|FAIL"

validIdsFields :: B.ByteString -> Bool
validIdsFields bytes = case takeField bytes of
  Just (recipient, rest1) -> case takeField rest1 of
    Just (sender, rest2) -> case takeField rest2 of
      Just (serverKey, rest3) ->
        not (B.null recipient) && not (B.null sender) &&
        B.length serverKey == 44 && B.null rest3
      _ -> False
    _ -> False
  _ -> False

takeField :: B.ByteString -> Maybe (B.ByteString, B.ByteString)
takeField bytes
  | B.null bytes = Nothing
  | B.length bytes < 1 + n = Nothing
  | otherwise = Just (B.take n (B.drop 1 bytes), B.drop (1 + n) bytes)
  where n = fromIntegral (B.head bytes)
