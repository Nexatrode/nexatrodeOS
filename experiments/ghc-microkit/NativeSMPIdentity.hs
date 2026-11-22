-- | SimpleXMQ's pinned server-identity check over a TLS certificate chain.
-- The expected SHA-256 fingerprint must come from a trusted SMP server address;
-- never derive it from the untrusted chain received during this connection.
module NativeSMPIdentity (validatePinnedServer) where

import qualified Data.ByteString as B
import qualified Data.X509 as X
import qualified Data.X509.CertificateStore as XS
import qualified Data.X509.Validation as XV

-- | Follow SimpleXMQ Transport.Client.validateCertificateChain: the identity
-- certificate is the second certificate, while the trust anchor is the last.
-- The DNS name is deliberately not checked for a pinned SimpleX identity.
validatePinnedServer :: B.ByteString -> (String, B.ByteString)
                     -> X.CertificateChain -> IO [XV.FailedReason]
validatePinnedServer expected service chain@(X.CertificateChain certs) =
  case certs of
    [] -> pure [XV.EmptyChain]
    [_] -> pure [XV.EmptyChain]
    [_, identity] -> check identity identity
    [_, identity, anchor] -> check identity anchor
    [_, identity, _, anchor] -> check identity anchor
    _ -> pure [XV.AuthorityTooDeep]
  where
    check identity anchor
      | XV.Fingerprint expected /= XV.getFingerprint identity X.HashSHA256 =
          pure [XV.UnknownCA]
      | otherwise =
          XV.validate X.HashSHA256 XV.defaultHooks checks
            (XS.makeCertificateStore [anchor]) noCache service chain
    checks = XV.defaultChecks { XV.checkFQHN = False }
    noCache = XV.ValidationCache
      (\_ _ _ -> pure XV.ValidationCacheUnknown)
      (\_ _ _ -> pure ())
