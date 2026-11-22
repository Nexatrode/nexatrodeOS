-- | Pinned SMP TLS over the protected network-PD capability.
-- Upstream Transport.Client uses a POSIX Socket for this handoff; a Microkit
-- guest supplies a TLS Backend instead.
module NativeSMPConnection (withPinnedSMPConnection) where

import Control.Exception (finally, onException)
import qualified Data.ByteString as B
import NativeNetwork (cancelSession)
import NativeSMPIdentity (validatePinnedServer)
import NativeTLSBackend (nativeTLSBackendForHost)
import qualified Network.TLS as TLS
import Network.TLS.Extra.Cipher (ciphersuite_default_det)

withPinnedSMPConnection :: String -> String -> B.ByteString -> Int ->
                           (TLS.Context -> IO a) -> IO a
withPinnedSMPConnection serverName service pin timeoutMs action =
  (`finally` cancelSession) $ do
    let defaults = TLS.defaultParamsClient serverName B.empty
        hooks = (TLS.clientHooks defaults)
          { TLS.onServerCertificate = \_ _ service chain ->
              validatePinnedServer pin service chain }
        supported = (TLS.clientSupported defaults)
          { TLS.supportedCiphers = ciphersuite_default_det }
        params = defaults { TLS.clientHooks = hooks
                          , TLS.clientSupported = supported }
    backend <- nativeTLSBackendForHost serverName service timeoutMs
    ctx <- TLS.contextNew backend params
    TLS.handshake ctx `onException` TLS.contextClose ctx
    action ctx `finally` TLS.contextClose ctx
