-- | Client-only Microkit compatibility surface for the upstream server module.
--
-- The SimpleX client does not load server credentials. Keep the upstream
-- server API typecheckable, but fail explicitly if server-certificate loading
-- is ever reached until the target has a filesystem-backed server runtime.
module Crypto.Store.X509 (readSignedObject) where

import Control.Exception (throwIO)

readSignedObject :: FilePath -> IO [a]
readSignedObject _ = throwIO $ userError "server certificate loading is unavailable in Microkit"
