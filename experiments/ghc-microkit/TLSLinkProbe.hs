-- | QEMU-only TLS fixture. Its CA and host-controlled RTC are test inputs;
-- this is not a production certificate or phone time source.
module Main where

import Crypto.Hash (Digest, SHA256, hash)
import Crypto.Random.Entropy (getEntropy)
import Control.Exception (SomeException, try)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as L
import Data.List (isInfixOf)
import Data.X509 (decodeSignedCertificate)
import qualified Data.X509 as X
import Data.X509.CertificateStore (makeCertificateStore)
import qualified Data.X509.Validation as XV
import NativeNetwork (IPv4, NetworkError(..), resolveIPv4, connectIPv4,
                      cancelSession)
import NativeSMPIdentity (validatePinnedServer)
import NativeTLSBackend (nativeTLSBackend)
import qualified Network.TLS as TLS
import Network.TLS.Extra.Cipher (ciphersuite_default_det)
import TLSFixtureCA (caDER)
import Time.System (timeCurrentP)
import Time.Types (ElapsedP(..), Elapsed(..), Seconds(..))

main :: IO ()
main = do
  putStrLn ("TLS_CRYPTO|SHA256|" ++ show (hash (B.pack [1, 2, 3]) :: Digest SHA256))
  let backend = nativeTLSBackend 200
  received <- TLS.backendRecv backend 0
  TLS.backendFlush backend
  putStrLn (if B.null received then "TLS_BACKEND|PASS" else "TLS_BACKEND|FAIL")
  first <- getEntropy 32 :: IO B.ByteString
  second <- getEntropy 32 :: IO B.ByteString
  putStrLn (if B.length first == 32 && B.length second == 32 &&
               B.any (/= 0) first && first /= second
            then "TLS_ENTROPY|PASS" else "TLS_ENTROPY|FAIL")
  let defaults = TLS.defaultParamsClient "personal-os.test" B.empty
      supported = (TLS.clientSupported defaults)
        { TLS.supportedCiphers = ciphersuite_default_det }
      params = defaults { TLS.clientSupported = supported }
  _ <- TLS.contextNew backend params
  putStrLn "TLS_CONTEXT|PASS"
  ElapsedP (Elapsed (Seconds seconds)) _ <- timeCurrentP
  putStrLn ("TLS_TIME|" ++ show seconds)
  runTestHandshake

runTestHandshake :: IO ()
runTestHandshake = do
  resolved <- resolveIPv4 "personal-os.test" 3000
  case resolved of
    Left Unavailable -> putStrLn "TLS_HANDSHAKE|SKIP"
    Right address -> do
      case decodeSignedCertificate caDER of
        Left err -> putStrLn ("TLS_HANDSHAKE|CA_FAIL|" ++ err)
        Right ca -> do
          opened <- connectIPv4 address 1241 3000
          case opened of
            Left err -> putStrLn ("TLS_HANDSHAKE|CONNECT_FAIL|" ++ show err)
            Right () -> do
              let defaults = TLS.defaultParamsClient "personal-os.test" B.empty
                  shared = (TLS.clientShared defaults)
                    { TLS.sharedCAStore = makeCertificateStore [ca] }
                  supported = (TLS.clientSupported defaults)
                    { TLS.supportedCiphers = ciphersuite_default_det }
                  params = defaults
                    { TLS.clientShared = shared, TLS.clientSupported = supported }
              ctx <- TLS.contextNew (nativeTLSBackend 5000) params
              outcome <- try $ do
                TLS.handshake ctx
                TLS.sendData ctx (L.fromStrict (B.pack [84, 76, 83, 45, 80, 73, 78, 71]))
                TLS.recvData ctx
              firstPassed <- case (outcome :: Either SomeException B.ByteString) of
                Left err -> putStrLn ("TLS_HANDSHAKE|FAIL|" ++ show err) >> pure False
                Right reply ->
                  if reply == B.pack [84, 76, 83, 45, 80, 73, 78, 71]
                    then putStrLn "TLS_HANDSHAKE|PASS" >> pure True
                    else putStrLn "TLS_HANDSHAKE|BAD_ECHO" >> pure False
              cancelSession
              wrongOpened <- connectIPv4 address 1241 3000
              case wrongOpened of
                Left err -> putStrLn ("TLS_WRONG_HOST|CONNECT_FAIL|" ++ show err)
                Right () -> do
                  let wrongDefaults = TLS.defaultParamsClient "wrong.personal-os.test" B.empty
                      wrongShared = (TLS.clientShared wrongDefaults)
                        { TLS.sharedCAStore = makeCertificateStore [ca] }
                      wrongSupported = (TLS.clientSupported wrongDefaults)
                        { TLS.supportedCiphers = ciphersuite_default_det }
                      wrongParams = wrongDefaults
                        { TLS.clientShared = wrongShared
                        , TLS.clientSupported = wrongSupported }
                  wrongCtx <- TLS.contextNew (nativeTLSBackend 5000) wrongParams
                  rejected <- try (TLS.handshake wrongCtx) :: IO (Either SomeException ())
                  case rejected of
                    Left err | "NameMismatch" `isInfixOf` show err ->
                      putStrLn "TLS_WRONG_HOST|REJECT"
                    Left err -> putStrLn ("TLS_WRONG_HOST|OTHER_FAIL|" ++ show err)
                    Right () -> putStrLn "TLS_WRONG_HOST|ACCEPTED"
                  cancelSession
              let XV.Fingerprint pin = XV.getFingerprint ca X.HashSHA256
              pinnedAttempt address pin "SMP_PIN" (not firstPassed)
              pinnedAttempt address (B.replicate 32 0) "SMP_WRONG_PIN" False
    Left err -> putStrLn ("TLS_HANDSHAKE|DNS_FAIL|" ++ show err)

pinnedAttempt :: IPv4 -> B.ByteString -> String -> Bool -> IO ()
pinnedAttempt address pin label expectExpired = do
  opened <- connectIPv4 address 1241 3000
  case opened of
    Left err -> putStrLn (label ++ "|CONNECT_FAIL|" ++ show err)
    Right () -> do
      -- SimpleX pins the identity certificate, so an alternate transport
      -- hostname must not override a valid pin.
      let defaults = TLS.defaultParamsClient "wrong.personal-os.test" B.empty
          hooks = (TLS.clientHooks defaults)
            { TLS.onServerCertificate = \_ _ service chain ->
                validatePinnedServer pin service chain }
          supported = (TLS.clientSupported defaults)
            { TLS.supportedCiphers = ciphersuite_default_det }
          params = defaults { TLS.clientHooks = hooks
                            , TLS.clientSupported = supported }
      ctx <- TLS.contextNew (nativeTLSBackend 5000) params
      result <- try $ do
        TLS.handshake ctx
        TLS.sendData ctx (L.fromStrict (B.pack [84, 76, 83, 45, 80, 73, 78, 71]))
        TLS.recvData ctx
      case (result :: Either SomeException B.ByteString) of
        Right bytes | label == "SMP_PIN" &&
                      bytes == B.pack [84, 76, 83, 45, 80, 73, 78, 71] ->
          putStrLn "SMP_PIN|PASS"
        Left err | label == "SMP_WRONG_PIN" &&
                   "UnknownCa" `isInfixOf` show err ->
          putStrLn "SMP_WRONG_PIN|REJECT"
        Left err | label == "SMP_PIN" && expectExpired &&
                   "CertificateExpired" `isInfixOf` show err ->
          putStrLn "SMP_PIN_EXPIRED|REJECT"
        Right _ -> putStrLn (label ++ "|BAD_ECHO_OR_ACCEPTED")
        Left err -> putStrLn (label ++ "|FAIL|" ++ show err)
      cancelSession
