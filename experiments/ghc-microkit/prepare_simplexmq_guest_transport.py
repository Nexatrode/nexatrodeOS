#!/usr/bin/env python3
"""Add a pinned, guest-only CPP boundary to the staged SimpleXMQ sources.

This keeps the upstream host transport intact while compiling the TLS backend
path without POSIX socket/SOCKS APIs or SQLite field instances. The transform
is intentionally strict: an upstream source change must be reviewed instead
of silently producing a different target build.
"""

from pathlib import Path
import json
import re


def replace_once(path: Path, old: str, new: str) -> None:
    source = path.read_text()
    count = source.count(old)
    if count != 1:
        raise RuntimeError(f"expected one occurrence in {path}: {old!r}; found {count}")
    path.write_text(source.replace(old, new))


def wrap_once(path: Path, old: str, *, macro: str = "SIMPLEX_MICROKIT") -> None:
    replace_once(path, old, f"#if !defined({macro})\n{old}#endif\n")


def prepare(root: Path) -> None:
    base = root / "Simplex/Messaging"
    crypto = base / "Crypto.hs"
    replace_once(
        crypto,
        "import Simplex.Messaging.Agent.Store.DB (Binary (..), FromField (..), ToField (..), blobFieldDecoder)\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Simplex.Messaging.Agent.Store.DB (Binary (..), FromField (..), ToField (..), blobFieldDecoder)\n"
        "#endif\n",
    )
    # Instances needed by the host SQLite/Postgres stores; transport uses the
    # crypto values and wire encodings, not their database representation.
    wrap_once(crypto, """instance ToField APrivateSignKey where toField = toField . Binary . encodePrivKey

instance ToField APublicVerifyKey where toField = toField . Binary . encodePubKey

instance ToField APrivateAuthKey where toField = toField . Binary . encodePrivKey

instance ToField APublicAuthKey where toField = toField . Binary . encodePubKey

instance ToField APrivateDhKey where toField = toField . Binary . encodePrivKey

instance ToField APublicDhKey where toField = toField . Binary . encodePubKey

instance AlgorithmI a => ToField (PrivateKey a) where toField = toField . Binary . encodePrivKey

instance AlgorithmI a => ToField (PublicKey a) where toField = toField . Binary . encodePubKey

instance ToField (DhSecret a) where toField = toField . Binary . dhBytes'

instance FromField APrivateSignKey where fromField = blobFieldDecoder decodePrivKey

instance FromField APublicVerifyKey where fromField = blobFieldDecoder decodePubKey

instance FromField APrivateAuthKey where fromField = blobFieldDecoder decodePrivKey

instance FromField APublicAuthKey where fromField = blobFieldDecoder decodePubKey

instance FromField APrivateDhKey where fromField = blobFieldDecoder decodePrivKey

instance FromField APublicDhKey where fromField = blobFieldDecoder decodePubKey

instance (Typeable a, AlgorithmI a) => FromField (PrivateKey a) where fromField = blobFieldDecoder decodePrivKey

instance (Typeable a, AlgorithmI a) => FromField (PublicKey a) where fromField = blobFieldDecoder decodePubKey

instance (Typeable a, AlgorithmI a) => FromField (DhSecret a) where fromField = blobFieldDecoder strDecode
""")
    replace_once(crypto,
        "  deriving newtype (FromField)\n\ninstance ToField Key where toField (Key s) = toField $ Binary s\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "  deriving newtype (FromField)\n\ninstance ToField Key where toField (Key s) = toField $ Binary s\n"
        "#endif\n")
    wrap_once(crypto, """instance ToField KeyHash where toField = toField . Binary . strEncode

instance FromField KeyHash where fromField = blobFieldDecoder $ parseAll strP
""")
    wrap_once(crypto, """instance (Typeable a, Eq a, Show a, ASN1Object a) => FromField (SignedObject a) where
#if defined(dbPostgres)
  fromField f dat = SignedObject <$> blobFieldDecoder X.decodeSignedObject f dat
#else
  fromField = fmap SignedObject . blobFieldDecoder X.decodeSignedObject
#endif

instance (Eq a, Show a, ASN1Object a) => ToField (SignedObject a) where
  toField (SignedObject s) = toField . Binary $ X.encodeSignedObject s
""")
    replace_once(crypto,
        "  deriving newtype (FromField)\n\ninstance ToField CbNonce where toField (CryptoBoxNonce s) = toField $ Binary s\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "  deriving newtype (FromField)\n\ninstance ToField CbNonce where toField (CryptoBoxNonce s) = toField $ Binary s\n"
        "#endif\n")
    replace_once(crypto,
        "  deriving newtype (FromField)\n\ninstance ToField SbKey where toField (SecretBoxKey s) = toField $ Binary s\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "  deriving newtype (FromField)\n\ninstance ToField SbKey where toField (SecretBoxKey s) = toField $ Binary s\n"
        "#endif\n")

    keepalive = base / "Transport/KeepAlive.hs"
    replace_once(keepalive, "{-# LANGUAGE CApiFFI #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE CApiFFI #-}\n")
    replace_once(keepalive,
        "    defaultKeepAliveOpts,\n    setSocketKeepAlive,\n",
        "    defaultKeepAliveOpts,\n#if !defined(SIMPLEX_MICROKIT)\n    setSocketKeepAlive,\n#endif\n")
    wrap_once(keepalive, "import Foreign.C (CInt (..))\nimport Network.Socket\n")
    start = "_SOL_TCP :: CInt\n"
    end = "$(J.deriveJSON defaultJSON ''KeepAliveOpts)\n"
    source = keepalive.read_text()
    if source.count(start) != 1 or source.count(end) != 1:
        raise RuntimeError(f"unexpected keep-alive source layout: {keepalive}")
    a, rest = source.split(start, 1)
    middle, b = rest.split(end, 1)
    keepalive.write_text(a + "#if !defined(SIMPLEX_MICROKIT)\n" + start + middle
                         + "#endif\n\n#if !defined(SIMPLEX_MICROKIT)\n" + end
                         + "#endif\n" + b)

    transport = base / "Transport.hs"
    replace_once(transport, "{-# LANGUAGE AllowAmbiguousTypes #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE AllowAmbiguousTypes #-}\n{-# LANGUAGE DeriveGeneric #-}\n")
    replace_once(transport, "import qualified Data.Aeson.TH as J\n",
                 "import qualified Data.Aeson.TH as J\nimport qualified Data.Aeson as Aeson\nimport GHC.Generics (Generic)\n")
    replace_once(transport,
        "    TEHandshake {handshakeErr :: HandshakeError}\n  deriving (Eq, Read, Show, Exception)",
        "    TEHandshake {handshakeErr :: HandshakeError}\n  deriving (Eq, Read, Show, Exception, Generic)")
    replace_once(transport,
        "    BAD_SERVICE\n  deriving (Eq, Read, Show, Exception)",
        "    BAD_SERVICE\n  deriving (Eq, Read, Show, Exception, Generic)")
    replace_once(transport, "    connectTLS,\n", "#if !defined(SIMPLEX_MICROKIT)\n    connectTLS,\n#endif\n")
    replace_once(transport, "import Network.Socket\n", "#if !defined(SIMPLEX_MICROKIT)\nimport Network.Socket\n#endif\n")
    replace_once(transport,
        "connectTLS :: T.TLSParams p => Maybe HostName -> TransportConfig -> p -> Socket -> IO T.Context\nconnectTLS host_ TransportConfig {logTLSErrors} params sock =\n  connectTLSWith host_ logTLSErrors (T.contextNew sock params)\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "connectTLS :: T.TLSParams p => Maybe HostName -> TransportConfig -> p -> Socket -> IO T.Context\n"
        "connectTLS host_ TransportConfig {logTLSErrors} params sock =\n"
        "  connectTLSWith host_ logTLSErrors (T.contextNew sock params)\n"
        "#endif\n")
    replace_once(transport,
        "connectTLSBackend :: T.TLSParams p => Maybe HostName -> TransportConfig -> p -> T.Backend -> IO T.Context\n",
        "connectTLSBackend :: T.TLSParams p => Maybe String -> TransportConfig -> p -> T.Backend -> IO T.Context\n")
    replace_once(transport,
        "connectTLSWith :: Maybe HostName -> Bool -> IO T.Context -> IO T.Context\n",
        "connectTLSWith :: Maybe String -> Bool -> IO T.Context -> IO T.Context\n")
    replace_once(transport,
        "$(J.deriveJSON (sumTypeJSON id) ''HandshakeError)\n\n$(J.deriveJSON (sumTypeJSON $ dropPrefix \"TE\") ''TransportError)\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "$(J.deriveJSON (sumTypeJSON id) ''HandshakeError)\n\n$(J.deriveJSON (sumTypeJSON $ dropPrefix \"TE\") ''TransportError)\n"
        "#else\n"
        "instance Aeson.ToJSON HandshakeError where\n  toJSON = Aeson.genericToJSON (sumTypeJSON id)\n  toEncoding = Aeson.genericToEncoding (sumTypeJSON id)\ninstance Aeson.FromJSON HandshakeError where\n  parseJSON = Aeson.genericParseJSON (sumTypeJSON id)\n"
        "instance Aeson.ToJSON TransportError where\n  toJSON = Aeson.genericToJSON (sumTypeJSON $ dropPrefix \"TE\")\n  toEncoding = Aeson.genericToEncoding (sumTypeJSON $ dropPrefix \"TE\")\ninstance Aeson.FromJSON TransportError where\n  parseJSON = Aeson.genericParseJSON (sumTypeJSON $ dropPrefix \"TE\")\n#endif\n")

    client = base / "Transport/Client.hs"
    replace_once(client, "{-# LANGUAGE DataKinds #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DataKinds #-}\n")
    replace_once(client, "import Data.IP\n",
                 "#if !defined(SIMPLEX_MICROKIT)\nimport Data.IP\n#endif\n")
    replace_once(client, "import Data.Char (isAsciiLower, isDigit, isHexDigit)\n",
                 "import Data.Char (isAsciiLower, isDigit, isHexDigit)\n"
                 "import Data.Bits (shiftL, shiftR, (.&.), (.|.))\n"
                 "import Data.List (intercalate)\nimport Numeric (readHex, showHex)\n")
    replace_once(client,
        "instance Encoding TransportHost where\n",
        """#if defined(SIMPLEX_MICROKIT)
ipv6Words :: (Word32, Word32, Word32, Word32) -> [Word32]
ipv6Words (a, b, c, d) = concatMap halves [a, b, c, d]
  where halves n = [n `shiftR` 16, n .&. 0xffff]

encodeIPv6Words :: (Word32, Word32, Word32, Word32) -> String
encodeIPv6Words = intercalate ":" . map (flip showHex "") . ipv6Words

parseIPv6Words :: String -> Either String (Word32, Word32, Word32, Word32)
parseIPv6Words input = do
  let (leftText, rightText, compressed) = case breakOnDoubleColon input of
        Just (left, right) -> (left, right, True)
        Nothing -> (input, "", False)
      leftParts = splitSingle leftText
      rightParts = if compressed then splitSingle rightText else []
  left <- traverse parseGroup leftParts
  right <- traverse parseGroup rightParts
  let missing = 8 - length left - length right
      groups = if compressed then left ++ replicate missing 0 ++ right else left
  if missing < 0 || (compressed && missing < 1) || length groups /= 8
    then Left "invalid IPv6 group count"
    else case groups of
      [a,b,c,d,e,f,g,h] -> Right (pair a b, pair c d, pair e f, pair g h)
      _ -> Left "invalid IPv6 group count"
  where
    splitSingle [] = []
    splitSingle s = case break (== ':') s of
      (a, []) -> [a]
      (a, _ : rest) -> a : splitSingle rest
    breakOnDoubleColon s = seek [] s
      where
        seek _ [] = Nothing
        seek acc (':' : ':' : rest) = Just (reverse acc, rest)
        seek acc (c : rest) = seek (c : acc) rest
    parseGroup [] = Left "empty IPv6 group"
    parseGroup s
      | length s > 4 || any (not . isHexDigit) s = Left "invalid IPv6 group"
      | otherwise = case readHex s of
          [(n, "")] -> Right n
          _ -> Left "invalid IPv6 group"
    pair hi lo = (hi `shiftL` 16) .|. lo
#endif

instance Encoding TransportHost where
""")
    replace_once(client,
        "    THIPv6 addr -> bshow $ toIPv6w addr\n",
        "#if defined(SIMPLEX_MICROKIT)\n    THIPv6 addr -> B.pack $ encodeIPv6Words addr\n"
        "#else\n    THIPv6 addr -> bshow $ toIPv6w addr\n#endif\n")
    replace_once(client,
        "        maybe (Left \"bad IPv6\") (Right . THIPv6 . fromIPv6w) . readMaybe . B.unpack <$?> ipv6StrP,\n",
        "#if defined(SIMPLEX_MICROKIT)\n        THIPv6 <$> (parseIPv6Words . B.unpack <$?> ipv6StrP),\n"
        "#else\n        maybe (Left \"bad IPv6\") (Right . THIPv6 . fromIPv6w) . readMaybe . B.unpack <$?> ipv6StrP,\n#endif\n")
    # Host socket/SOCKS exports and imports are absent from the Microkit build.
    replace_once(client,
        "  ( runTransportClient,\n    runTLSTransportClient,\n",
        "  (\n#if !defined(SIMPLEX_MICROKIT)\n    runTransportClient,\n    runTLSTransportClient,\n#endif\n")
    replace_once(client, "    defaultSMPPort,\n    defaultTcpConnectTimeout,\n",
        "    defaultSMPPort,\n    defaultTcpConnectTimeout,\n")
    replace_once(client,
        "    defaultSocksProxyWithAuth,\n    defaultSocksProxy,\n    defaultSocksHost,\n",
        "#if !defined(SIMPLEX_MICROKIT)\n    defaultSocksProxyWithAuth,\n    defaultSocksProxy,\n    defaultSocksHost,\n#endif\n")
    replace_once(client,
        "    SocksProxy (..),\n    SocksProxyWithAuth (..),\n    SocksAuth (..),\n",
        "    SocksProxy (..),\n    SocksProxyWithAuth (..),\n    SocksAuth (..),\n")
    replace_once(client, "import Network.Socket\nimport Network.Socks5\n",
        "#if !defined(SIMPLEX_MICROKIT)\nimport Network.Socket hiding (HostName, ServiceName)\nimport Network.Socks5\n#endif\n")
    replace_once(client, "import GHC.IO.Exception (IOErrorType (..))\n", "#if !defined(SIMPLEX_MICROKIT)\nimport GHC.IO.Exception (IOErrorType (..))\n#endif\n")
    replace_once(client, "import System.IO.Error\n", "#if !defined(SIMPLEX_MICROKIT)\nimport System.IO.Error\n#endif\n")
    replace_once(client, "import UnliftIO.Exception (IOException)\n",
        "#if !defined(SIMPLEX_MICROKIT)\nimport UnliftIO.Exception (IOException)\n#endif\n")
    replace_once(client,
        "import UnliftIO.STM\n\ndata TransportHost\n",
        "import UnliftIO.STM\n\ntype HostName = String\ntype ServiceName = String\n\ndata TransportHost\n")
    replace_once(client,
        "data TransportClientConfig = TransportClientConfig\n  { socksProxy :: Maybe SocksProxy,\n    tcpConnectTimeout :: Int,\n",
        "data TransportClientConfig = TransportClientConfig\n  { socksProxy :: Maybe SocksProxy,\n    tcpConnectTimeout :: Int,\n")
    replace_once(client,
        "  TransportClientConfig\n    { socksProxy = Nothing,\n      tcpConnectTimeout = defaultTcpConnectTimeout,\n",
        "  TransportClientConfig\n    { socksProxy = Nothing,\n      tcpConnectTimeout = defaultTcpConnectTimeout,\n")
    host_start = "-- | Connect to passed TCP host:port and pass handle to the client.\n"
    backend_start = "-- | Use the same TLS parameters and peer-certificate handoff as the socket\n"
    source = client.read_text()
    if source.count(host_start) != 1 or source.count(backend_start) != 1:
        raise RuntimeError(f"unexpected transport client layout: {client}")
    pre, rest = source.split(host_start, 1)
    host, post = rest.split(backend_start, 1)
    client.write_text(pre + "#if !defined(SIMPLEX_MICROKIT)\n" + host_start + host
                       + "#endif\n\n" + backend_start + post)
    host_helpers_start = "connectTCPClient :: HostName -> ServiceName -> IO Socket\n"
    tls_params_start = "mkTLSClientParams :: T.Supported -> Maybe XS.CertificateStore -> HostName -> ServiceName ->"
    source = client.read_text()
    if source.count(host_helpers_start) != 1 or source.count(tls_params_start) != 1:
        raise RuntimeError(f"unexpected socket helper layout: {client}")
    pre, rest = source.split(host_helpers_start, 1)
    helpers, post = rest.split(tls_params_start, 1)
    client.write_text(pre + "#if !defined(SIMPLEX_MICROKIT)\n" + host_helpers_start
                       + helpers + "#endif\n\n" + tls_params_start + post)
    replace_once(client,
        "#endif\n\nmkTLSClientParams :: T.Supported",
        '''#endif

#if defined(SIMPLEX_MICROKIT)
defaultSMPPort :: String
defaultSMPPort = "5223"

newtype SocksProxy = SocksProxy ByteString
  deriving (Eq, Show)

data SocksProxyWithAuth = SocksProxyWithAuth SocksAuth SocksProxy
  deriving (Eq, Show)

data SocksAuth
  = SocksAuthUsername {username :: ByteString, password :: ByteString}
  | SocksAuthNull
  | SocksIsolateByAuth
  deriving (Eq, Show)

instance StrEncoding SocksProxy where
  strEncode (SocksProxy proxy) = proxy
  strP = SocksProxy <$> A.takeWhile1 (/= ' ')

instance StrEncoding SocksProxyWithAuth where
  strEncode (SocksProxyWithAuth auth proxy) = strEncode auth <> strEncode proxy
  strP = SocksProxyWithAuth <$> strP <*> strP

instance ToJSON SocksProxyWithAuth where
  toJSON = strToJSON
  toEncoding = strToJEncoding

instance FromJSON SocksProxyWithAuth where
  parseJSON = strParseJSON "SocksProxyWithAuth"

instance StrEncoding SocksAuth where
  strEncode = \\case
    SocksAuthUsername {username, password} -> username <> ":" <> password <> "@"
    SocksAuthNull -> "@"
    SocksIsolateByAuth -> ""
  strP = usernameP <|> (SocksAuthNull <$ A.char '@') <|> pure SocksIsolateByAuth
    where
      usernameP = do
        username <- A.takeTill (== ':') <* A.char ':'
        password <- A.takeTill (== '@') <* A.char '@'
        pure SocksAuthUsername {username, password}
#endif

mkTLSClientParams :: T.Supported''')
    replace_once(client,
        "runTLSBackendTransportClient :: T.Supported -> Maybe XS.CertificateStore -> TransportClientConfig -> TransportHost -> ServiceName -> Maybe C.KeyHash -> IO T.Backend -> (TLS 'TClient -> IO a) -> IO a\n",
        "runTLSBackendTransportClient :: T.Supported -> Maybe XS.CertificateStore -> TransportClientConfig -> TransportHost -> String -> Maybe C.KeyHash -> IO T.Backend -> (TLS 'TClient -> IO a) -> IO a\n")
    replace_once(client,
        "mkTLSClientParams :: T.Supported -> Maybe XS.CertificateStore -> HostName -> ServiceName -> Maybe C.KeyHash -> Maybe T.Credential -> IORef Bool -> Maybe [ALPN] -> Bool -> TMVar (Maybe X.CertificateChain) -> T.ClientParams\n",
        "mkTLSClientParams :: T.Supported -> Maybe XS.CertificateStore -> String -> String -> Maybe C.KeyHash -> Maybe T.Credential -> IORef Bool -> Maybe [ALPN] -> Bool -> TMVar (Maybe X.CertificateChain) -> T.ClientParams\n")

    protocol_client = base / "Client.hs"
    # Keep the upstream Client configuration/API types available to guest
    # code, but represent SOCKS settings as inert values. The Microkit TLS
    # backend path does not implement SOCKS or POSIX socket opening.
    replace_once(protocol_client,
        "{-# LANGUAGE DataKinds #-}\n",
        "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DataKinds #-}\n")
    replace_once(protocol_client,
        "import Network.Socket (HostName, ServiceName)\nimport Network.Socks5 (SocksCredentials (..))\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Network.Socket (HostName, ServiceName)\n"
        "import Network.Socks5 (SocksCredentials (..))\n"
        "#endif\n")
    replace_once(protocol_client,
        "import System.Timeout (timeout)\n",
        "import System.Timeout (timeout)\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "type HostName = String\n"
        "type ServiceName = String\n"
        "data SocksCredentials = SocksCredentials ByteString ByteString\n"
        "#endif\n")
    replace_once(protocol_client,
        "      | B.null $ bs corrId = sendMsg $ STEvent clientResp\n",
        "      | B.null $ bs corrId = sendMsg $ STEvent clientResp\n"
        )
    replace_once(protocol_client,
        "import Simplex.Messaging.Transport.Client (SocksAuth (..), SocksProxyWithAuth (..), TransportClientConfig (..), TransportHost (..), defaultSMPPort, runTLSBackendTransportClient, runTransportClient)\n",
        "import Simplex.Messaging.Transport.Client (SocksAuth (..), SocksProxyWithAuth (..), TransportClientConfig (..), TransportHost (..), defaultSMPPort, runTLSBackendTransportClient\n"
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "  , runTransportClient\n"
        "#endif\n"
        "  )\n")
    # The guest port is already a String. show on that String quotes it, so a
    # peer relay that omits the default port is dialed as "5223" and never opens.
    replace_once(protocol_client,
        "    { defaultTransport = (show defaultSMPPort, transport @TLS),\n",
        "    { defaultTransport = (\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "        defaultSMPPort\n"
        "#else\n"
        "        show defaultSMPPort\n"
        "#endif\n"
        "        , transport @TLS),\n")
    replace_once(protocol_client,
        "          runConnection = case backendConnector of\n            Nothing -> runTransportClient tcConfig socksCreds useHost port' (Just $ keyHash srv) (client t c cVar)\n            Just connector ->\n",
        "          runConnection = case backendConnector of\n#if defined(SIMPLEX_MICROKIT)\n            Nothing -> E.throwIO $ userError \"POSIX transport disabled; use getProtocolClientWithTLSBackend\"\n#else\n            Nothing -> runTransportClient tcConfig socksCreds useHost port' (Just $ keyHash srv) (client t c cVar)\n#endif\n            Just connector ->\n")
    replace_once(protocol_client,
        "import Simplex.Messaging.Transport.HTTP2 (httpALPN11)\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Simplex.Messaging.Transport.HTTP2 (httpALPN11)\n"
        "#endif\n")
    replace_once(protocol_client,
        "    useALPN = if web then Just [httpALPN11] else clientALPN\n",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "    useALPN = if web then Just [\"http/1.1\"] else clientALPN\n"
        "#else\n"
        "    useALPN = if web then Just [httpALPN11] else clientALPN\n"
        "#endif\n")
    wrap_once(protocol_client,
        "$(J.deriveJSON (enumJSON $ dropPrefix \"HM\") ''HostMode)\n\n"
        "$(J.deriveJSON (enumJSON $ dropPrefix \"SM\") ''SocksMode)\n\n"
        "$(J.deriveJSON (enumJSON $ dropPrefix \"TSM\") ''TransportSessionMode)\n\n"
        "$(J.deriveJSON (enumJSON $ dropPrefix \"SPM\") ''SMPProxyMode)\n\n"
        "$(J.deriveJSON (enumJSON $ dropPrefix \"SPF\") ''SMPProxyFallback)\n\n"
        "$(J.deriveJSON (enumJSON $ dropPrefix \"SWP\") ''SMPWebPortServers)\n\n"
        "$(J.deriveJSON defaultJSON ''NetworkTimeout)\n\n"
        "$(J.deriveJSON defaultJSON ''NetworkConfig)\n\n"
        "$(J.deriveJSON (sumTypeJSON $ dropPrefix \"Proxy\") ''ProxyClientError)\n\n"
        "$(J.deriveJSON defaultJSON ''TBQueueInfo)\n")
    protocol_types = base / "Protocol/Types.hs"
    replace_once(protocol_types, "{-# LANGUAGE FlexibleInstances #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DeriveGeneric #-}\n{-# LANGUAGE FlexibleInstances #-}\n")
    replace_once(protocol_types,
        "import Data.Int (Int64)\n",
        "import Data.Int (Int64)\nimport qualified Data.Aeson as Aeson\nimport GHC.Generics (Generic)\n")
    replace_once(protocol_types,
        "  deriving (Eq, Show)\n\n$(J.deriveJSON defaultJSON ''ClientNotice)\n",
        "  deriving (Eq, Show, Generic)\n\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "instance Aeson.ToJSON ClientNotice where\n"
        "  toJSON = Aeson.genericToJSON defaultJSON\n"
        "  toEncoding = Aeson.genericToEncoding defaultJSON\n"
        "instance Aeson.FromJSON ClientNotice where\n"
        "  parseJSON = Aeson.genericParseJSON defaultJSON\n"
        "#else\n$(J.deriveJSON defaultJSON ''ClientNotice)\n#endif\n")

    # Protocol and queue-info types are shared with server/database code in
    # upstream SimpleXMQ. The guest keeps their wire/API representations while
    # omitting only SQLite field conversions and the socket type alias.
    protocol = base / "Protocol.hs"
    replace_once(protocol, "{-# LANGUAGE DataKinds #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DataKinds #-}\n{-# LANGUAGE DeriveGeneric #-}\n")
    replace_once(protocol, "import qualified Data.Aeson as J\n",
                 "import qualified Data.Aeson as J\nimport GHC.Generics (Generic)\n")
    for old, new in (
        ("data MsgFlags = MsgFlags {notification :: Bool}\n  deriving (Eq, Show)",
         "data MsgFlags = MsgFlags {notification :: Bool}\n  deriving (Eq, Show, Generic)"),
        ("  deriving (Eq, Show)\n\n-- | Name resolution error", "  deriving (Eq, Show, Generic)\n\n-- | Name resolution error"),
        ("  deriving (Eq, Show)\n\ninstance StrEncoding ErrorType", "  deriving (Eq, Show, Generic)\n\ninstance StrEncoding ErrorType"),
        ("  deriving (Eq, Show)\n\n-- | SMP server errors.", "  deriving (Eq, Show, Generic)\n\n-- | SMP server errors."),
        ("  deriving (Eq, Read, Show, Exception)\n\ndata NetworkError", "  deriving (Eq, Read, Show, Exception, Generic)\n\ndata NetworkError"),
        ("  deriving (Eq, Read, Show)\n\ntoNetworkError ::", "  deriving (Eq, Read, Show, Generic)\n\ntoNetworkError ::"),
    ):
        replace_once(protocol, old, new)
    replace_once(protocol,
        "  deriving (Eq, Read, Show)\n\ndata ProxyError\n",
        "  deriving (Eq, Read, Show, Generic)\n\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "instance ToJSON CommandError where\n"
        "  toJSON = J.genericToJSON (sumTypeJSON id)\n"
        "  toEncoding = J.genericToEncoding (sumTypeJSON id)\n"
        "instance FromJSON CommandError where\n"
        "  parseJSON = J.genericParseJSON (sumTypeJSON id)\n"
        "#endif\n\ndata ProxyError\n")
    replace_once(protocol,
        "  deriving (Eq, Show)\n\ndata BlockingReason = BRSpam | BRContent\n  deriving (Eq, Show)\n",
        "  deriving (Eq, Show, Generic)\n\ndata BlockingReason = BRSpam | BRContent\n  deriving (Eq, Show, Generic)\n\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "instance ToJSON BlockingInfo where\n"
        "  toJSON = J.genericToJSON defaultJSON\n"
        "  toEncoding = J.genericToEncoding defaultJSON\n"
        "instance FromJSON BlockingInfo where\n"
        "  parseJSON = J.genericParseJSON defaultJSON\n"
        "#endif\n")
    replace_once(protocol, "import Network.Socket (ServiceName)\n",
                 "#if !defined(SIMPLEX_MICROKIT)\nimport Network.Socket (ServiceName)\n#endif\n")
    replace_once(protocol,
        "import Simplex.Messaging.Agent.Store.DB (Binary (..), FromField (..), ToField (..))\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Simplex.Messaging.Agent.Store.DB (Binary (..), FromField (..), ToField (..))\n"
        "#endif\n")
    replace_once(protocol,
        "import Simplex.Messaging.Version.Internal\n",
        "import Simplex.Messaging.Version.Internal\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "type ServiceName = String\n"
        "#endif\n")
    replace_once(protocol,
        "  deriving newtype (FromField, StrEncoding)\n",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "  deriving newtype (StrEncoding)\n"
        "#else\n"
        "  deriving newtype (FromField, StrEncoding)\n"
        "#endif\n")
    replace_once(protocol,
        "instance ToField EncDataBytes where\n  toField (EncDataBytes s) = toField (Binary s)\n  {-# INLINE toField #-}\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "instance ToField EncDataBytes where\n  toField (EncDataBytes s) = toField (Binary s)\n  {-# INLINE toField #-}\n"
        "#endif\n")
    replace_once(protocol,
        "  deriving newtype (Encoding, FromField)\n\ninstance ToField IdsHash where\n  toField (IdsHash s) = toField (Binary s)\n  {-# INLINE toField #-}\n",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "  deriving newtype (Encoding)\n"
        "#else\n"
        "  deriving newtype (Encoding, FromField)\n\n"
        "instance ToField IdsHash where\n  toField (IdsHash s) = toField (Binary s)\n  {-# INLINE toField #-}\n"
        "#endif\n")
    wrap_once(protocol,
        "$(J.deriveJSON defaultJSON ''MsgFlags)\n\n"
        "$(J.deriveJSON (sumTypeJSON id) ''CommandError)\n\n"
        "$(J.deriveToJSON (sumTypeJSON $ dropPrefix \"NE\") ''NetworkError)\n\n"
        "instance FromJSON NetworkError where\n"
        "  parseJSON = $(J.mkParseJSON (sumTypeJSON $ dropPrefix \"NE\") ''NetworkError)\n"
        "  omittedField = Just NEFailedError\n\n"
        "$(J.deriveJSON (sumTypeJSON id) ''BrokerErrorType)\n\n"
        "$(J.deriveJSON defaultJSON ''BlockingInfo)\n\n"
        "-- run deriveJSON in one TH splice to allow mutual instance\n"
        "$(concat <$> mapM @[] (J.deriveJSON (sumTypeJSON id)) [''ProxyError, ''NameErrorType, ''ErrorType])\n")
    replace_once(protocol,
        "$(concat <$> mapM @[] (J.deriveJSON (sumTypeJSON id)) [''ProxyError, ''NameErrorType, ''ErrorType])\n#endif",
        "$(concat <$> mapM @[] (J.deriveJSON (sumTypeJSON id)) [''ProxyError, ''NameErrorType, ''ErrorType])\n"
        "#else\n"
        "instance ToJSON MsgFlags where\n  toJSON = J.genericToJSON defaultJSON\n  toEncoding = J.genericToEncoding defaultJSON\ninstance FromJSON MsgFlags where\n  parseJSON = J.genericParseJSON defaultJSON\n"
        "instance ToJSON NetworkError where\n  toJSON = J.genericToJSON (sumTypeJSON $ dropPrefix \"NE\")\n  toEncoding = J.genericToEncoding (sumTypeJSON $ dropPrefix \"NE\")\ninstance FromJSON NetworkError where\n  parseJSON = J.genericParseJSON (sumTypeJSON $ dropPrefix \"NE\")\n  omittedField = Just NEFailedError\n"
        "instance ToJSON BrokerErrorType where\n  toJSON = J.genericToJSON (sumTypeJSON id)\n  toEncoding = J.genericToEncoding (sumTypeJSON id)\ninstance FromJSON BrokerErrorType where\n  parseJSON = J.genericParseJSON (sumTypeJSON id)\n"
        "instance ToJSON ProxyError where\n  toJSON = J.genericToJSON (sumTypeJSON id)\n  toEncoding = J.genericToEncoding (sumTypeJSON id)\ninstance FromJSON ProxyError where\n  parseJSON = J.genericParseJSON (sumTypeJSON id)\n"
        "instance ToJSON NameErrorType where\n  toJSON = J.genericToJSON (sumTypeJSON id)\n  toEncoding = J.genericToEncoding (sumTypeJSON id)\ninstance FromJSON NameErrorType where\n  parseJSON = J.genericParseJSON (sumTypeJSON id)\n"
        "instance ToJSON ErrorType where\n  toJSON = J.genericToJSON (sumTypeJSON id)\n  toEncoding = J.genericToEncoding (sumTypeJSON id)\ninstance FromJSON ErrorType where\n  parseJSON = J.genericParseJSON (sumTypeJSON id)\n"
        "#endif")

    # Keep a local copy in the protocol-only target, so that this focused
    # compile does not pull the full socket/HTTP2 messaging client closure.
    agent_protocol = base / "Agent/Protocol.hs"
    replace_once(agent_protocol, "{-# LANGUAGE DataKinds #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DataKinds #-}\n")
    replace_once(
        agent_protocol,
        "    AgentErrorType (..),\n",
        "    AgentErrorType (..),\n#if defined(SIMPLEX_MICROKIT)\n    XFTPErrorType (..),\n#endif\n",
    )
    replace_once(
        agent_protocol,
        "{-# LANGUAGE DataKinds #-}\n",
        "{-# LANGUAGE DataKinds #-}\n{-# LANGUAGE DeriveGeneric #-}\n",
    )
    replace_once(
        agent_protocol,
        "import Data.Foldable (find)\n",
        "import Data.Foldable (find)\n"
        "import qualified Data.Aeson as GuestAeson\n"
        "import GHC.Generics (Generic)\n",
    )
    replace_once(
        agent_protocol,
        "import Simplex.Messaging.Client (ProxyClientError)\n",
        "",
    )
    replace_once(
        agent_protocol,
        "import Simplex.FileTransfer.Transport (XFTPErrorType)\n",
        "",
    )
    # Agent.Protocol only needs FileErrorType for its FILE error constructor.
    # Importing FileTransfer.Types drags the full file-transfer client and its
    # HTTP/2 transport into the SMP protocol build, so keep this wire schema
    # local to the staged protocol source as well.
    replace_once(
        agent_protocol,
        "import Simplex.FileTransfer.Types (FileErrorType)\n",
        "",
    )
    replace_once(
        agent_protocol,
        "-- SMP agent protocol version history:\n",
        "-- Target-only copy of Client.ProxyClientError. Keep the upstream\n"
        "-- constructor and JSON tags stable; transport behavior remains in Client.hs.\n"
        "data ProxyClientError\n"
        "  = ProxyProtocolError {protocolErr :: ErrorType}\n"
        "  | ProxyUnexpectedResponse {responseStr :: String}\n"
        "  | ProxyResponseError {responseErr :: ErrorType}\n"
        "  deriving (Eq, Show, Exception)\n\n"
        "deriving stock instance Generic ProxyClientError\n\n"
        "instance ToJSON ProxyClientError where\n"
        "  toJSON = GuestAeson.genericToJSON (sumTypeJSON $ dropPrefix \"Proxy\")\n"
        "  toEncoding = GuestAeson.genericToEncoding (sumTypeJSON $ dropPrefix \"Proxy\")\n"
        "instance FromJSON ProxyClientError where\n"
        "  parseJSON = GuestAeson.genericParseJSON (sumTypeJSON $ dropPrefix \"Proxy\")\n\n"
        "-- Protocol-facing XFTP errors, copied from FileTransfer.Transport so\n"
        "-- parsing the agent protocol does not compile the file-transfer backend.\n"
        "data XFTPErrorType\n"
        "  = XFTP_BLOCK | XFTP_SESSION | XFTP_HANDSHAKE | XFTP_CMD {cmdErr :: SMP.CommandError} | XFTP_AUTH\n"
        "  | XFTP_BLOCKED {blockInfo :: SMP.BlockingInfo} | XFTP_SIZE | XFTP_QUOTA | XFTP_DIGEST | XFTP_CRYPTO\n"
        "  | XFTP_NO_FILE | XFTP_HAS_FILE | XFTP_FILE_IO | XFTP_TIMEOUT | XFTP_INTERNAL | XFTP_DUPLICATE_\n"
        "  deriving (Eq, Show)\n\n"
        "deriving stock instance Generic XFTPErrorType\n\n"
        "instance ToJSON XFTPErrorType where\n"
        "  toJSON = GuestAeson.genericToJSON (sumTypeJSON $ dropPrefix \"XFTP_\")\n"
        "  toEncoding = GuestAeson.genericToEncoding (sumTypeJSON $ dropPrefix \"XFTP_\")\n"
        "instance FromJSON XFTPErrorType where\n"
        "  parseJSON = GuestAeson.genericParseJSON (sumTypeJSON $ dropPrefix \"XFTP_\")\n\n"
        "-- File-transfer error tags used by the SMP agent protocol. Keep in\n"
        "-- sync with Simplex.FileTransfer.Types.FileErrorType.\n"
        "data FileErrorType\n"
        "  = FET_NOT_APPROVED | FET_SIZE | FET_REDIRECT {redirectError :: String}\n"
        "  | FET_FILE_IO {fileIOError :: String} | FET_NO_FILE\n"
        "  deriving (Eq, Show)\n\n"
        "deriving stock instance Generic FileErrorType\n\n"
        "instance StrEncoding FileErrorType where\n"
        "  strP = A.takeTill (== ' ') >>= \\case\n"
        "    \"NOT_APPROVED\" -> pure FET_NOT_APPROVED\n"
        "    \"SIZE\" -> pure FET_SIZE\n"
        "    \"REDIRECT\" -> FET_REDIRECT <$> (A.space *> textP)\n"
        "    \"FILE_IO\" -> FET_FILE_IO <$> (A.space *> textP)\n"
        "    \"NO_FILE\" -> pure FET_NO_FILE\n"
        "    _ -> fail \"bad FileErrorType\"\n"
        "  strEncode = \\case\n"
        "    FET_NOT_APPROVED -> \"NOT_APPROVED\"\n"
        "    FET_SIZE -> \"SIZE\"\n"
        "    FET_REDIRECT e -> \"REDIRECT \" <> encodeUtf8 (T.pack e)\n"
        "    FET_FILE_IO e -> \"FILE_IO \" <> encodeUtf8 (T.pack e)\n"
        "    FET_NO_FILE -> \"NO_FILE\"\n\n"
        "instance ToJSON FileErrorType where\n"
        "  toJSON = GuestAeson.genericToJSON (sumTypeJSON $ dropPrefix \"FET_\")\n"
        "  toEncoding = GuestAeson.genericToEncoding (sumTypeJSON $ dropPrefix \"FET_\")\n"
        "instance FromJSON FileErrorType where\n"
        "  parseJSON = GuestAeson.genericParseJSON (sumTypeJSON $ dropPrefix \"FET_\")\n\n"
        "-- SMP agent protocol version history:\n",
    )

    # Aeson TH emits references to internal, non-exposed modules which cannot
    # be recompiled by the freestanding cross GHC. Aeson generic instances use
    # the same Options values and keep the wire representation while avoiding
    # TH execution. Derive Generic for the protocol records and enums here.
    agent_protocol_generics = (
        "deriving stock instance Generic QueueStatus\n"
        "deriving stock instance Generic SubscriptionStatus\n"
        "deriving stock instance Generic RcvQueueInfo\n"
        "deriving stock instance Generic SndQueueInfo\n"
        "deriving stock instance Generic ConnectionStats\n"
        "deriving stock instance Generic MsgErrorType\n"
        "deriving stock instance Generic MsgIntegrity\n"
        "deriving stock instance Generic CommandErrorType\n"
        "deriving stock instance Generic ConnectionErrorType\n"
        "deriving stock instance Generic AgentCryptoError\n"
        "deriving stock instance Generic DroppedMsg\n"
        "deriving stock instance Generic SMPAgentError\n"
        "deriving stock instance Generic AgentErrorType\n"
        "deriving stock instance Generic QueueDirection\n"
        "deriving stock instance Generic SwitchPhase\n"
        "deriving stock instance Generic (CreatedConnLink m)\n\n"
    )
    replace_once(
        agent_protocol,
        "$(J.deriveJSON (enumJSON fstToLower) ''QueueStatus)\n",
        agent_protocol_generics
        + "$(J.deriveJSON (enumJSON fstToLower) ''QueueStatus)\n",
    )

    def generic_json(type_name: str, options: str, context: str = "") -> str:
        constraint = f"{context} => " if context else ""
        return (
            f"instance {constraint}ToJSON {type_name} where\n"
            f"  toJSON = GuestAeson.genericToJSON ({options})\n"
            f"  toEncoding = GuestAeson.genericToEncoding ({options})\n"
            f"instance {constraint}FromJSON {type_name} where\n"
            f"  parseJSON = GuestAeson.genericParseJSON ({options})\n"
        )

    for splice, type_name, options in (
        ("$(J.deriveJSON (enumJSON fstToLower) ''QueueStatus)\n", "QueueStatus", "enumJSON fstToLower"),
        ("$(J.deriveJSON (sumTypeJSON $ dropPrefix \"SS\") ''SubscriptionStatus)\n", "SubscriptionStatus", "sumTypeJSON $ dropPrefix \"SS\""),
        ("$(J.deriveJSON defaultJSON ''RcvQueueInfo)\n", "RcvQueueInfo", "defaultJSON"),
        ("$(J.deriveJSON defaultJSON ''SndQueueInfo)\n", "SndQueueInfo", "defaultJSON"),
        ("$(J.deriveJSON defaultJSON ''ConnectionStats)\n", "ConnectionStats", "defaultJSON"),
        ("$(J.deriveJSON (sumTypeJSON fstToLower) ''MsgErrorType)\n", "MsgErrorType", "sumTypeJSON fstToLower"),
        ("$(J.deriveJSON (sumTypeJSON fstToLower) ''MsgIntegrity)\n", "MsgIntegrity", "sumTypeJSON fstToLower"),
        ("$(J.deriveJSON (sumTypeJSON id) ''CommandErrorType)\n", "CommandErrorType", "sumTypeJSON id"),
        ("$(J.deriveJSON (sumTypeJSON id) ''ConnectionErrorType)\n", "ConnectionErrorType", "sumTypeJSON id"),
        ("$(J.deriveJSON (sumTypeJSON id) ''AgentCryptoError)\n", "AgentCryptoError", "sumTypeJSON id"),
        ("$(J.deriveJSON defaultJSON ''DroppedMsg)\n", "DroppedMsg", "defaultJSON"),
        ("$(J.deriveJSON (sumTypeJSON id) ''SMPAgentError)\n", "SMPAgentError", "sumTypeJSON id"),
        ("$(J.deriveJSON (sumTypeJSON id) ''AgentErrorType)\n", "AgentErrorType", "sumTypeJSON id"),
        ("$(J.deriveJSON (enumJSON $ dropPrefix \"QD\") ''QueueDirection)\n", "QueueDirection", "enumJSON $ dropPrefix \"QD\""),
        ("$(J.deriveJSON (enumJSON $ dropPrefix \"SP\") ''SwitchPhase)\n", "SwitchPhase", "enumJSON $ dropPrefix \"SP\""),
    ):
        replace_once(agent_protocol, splice, generic_json(type_name, options))

    replace_once(
        agent_protocol,
        "parseJSON = $(J.mkParseJSON defaultJSON ''CreatedConnLink)",
        "parseJSON = GuestAeson.genericParseJSON defaultJSON",
    )
    replace_once(
        agent_protocol,
        "toEncoding = $(J.mkToEncoding defaultJSON ''CreatedConnLink)",
        "toEncoding = GuestAeson.genericToEncoding defaultJSON",
    )
    replace_once(
        agent_protocol,
        "toJSON = $(J.mkToJSON defaultJSON ''CreatedConnLink)",
        "toJSON = GuestAeson.genericToJSON defaultJSON",
    )

    # Keep FileTransfer.Protocol's wire codecs and protocol instances while
    # separating its authentication helper and handshake/error vocabulary from
    # the full socket/HTTP2 file-transfer transport.
    file_protocol = root / "Simplex/FileTransfer/Protocol.hs"
    replace_once(file_protocol, "{-# LANGUAGE DataKinds #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DataKinds #-}\n{-# LANGUAGE DeriveGeneric #-}\n")
    replace_once(file_protocol, "import qualified Data.Aeson.TH as J\n",
                 "import qualified Data.Aeson as J\nimport GHC.Generics (Generic)\n")
    replace_once(file_protocol,
        "data FileParty = FRecipient | FSender\n  deriving (Eq, Show)\n",
        "data FileParty = FRecipient | FSender\n  deriving (Eq, Show, Generic)\n")
    replace_once(file_protocol,
        "$(J.deriveJSON (enumJSON $ dropPrefix \"F\") ''FileParty)",
        "instance J.ToJSON FileParty where\n"
        "  toJSON = J.genericToJSON (enumJSON $ dropPrefix \"F\")\n"
        "  toEncoding = J.genericToEncoding (enumJSON $ dropPrefix \"F\")\n"
        "instance J.FromJSON FileParty where\n"
        "  parseJSON = J.genericParseJSON (enumJSON $ dropPrefix \"F\")")
    replace_once(
        file_protocol,
        "import Simplex.FileTransfer.Transport (XFTPErrorType (..), XFTPVersion, blockedFilesXFTPVersion, xftpClientHandshakeStub)\n",
        "import Simplex.FileTransfer.Transport.Types (XFTPErrorType (..), XFTPVersion, blockedFilesXFTPVersion, xftpClientHandshakeStub)\n",
    )
    replace_once(
        file_protocol,
        "import Simplex.Messaging.Client (authTransmission)\n",
        "import Simplex.Messaging.Client.Auth (authTransmission)\n",
    )

    # The event protocol only needs YAMLFileDescription's JSON-compatible
    # representation. JSON is a YAML subset; the guest shim emits JSON bytes
    # and deliberately accepts JSON input only, avoiding libyaml in the first
    # message-capable target.
    description = root / "Simplex/FileTransfer/Description.hs"
    replace_once(description, "{-# LANGUAGE DataKinds #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DataKinds #-}\n{-# LANGUAGE DeriveGeneric #-}\n")
    replace_once(description, "import qualified Data.Aeson.TH as J\n",
                 "import qualified Data.Aeson.TH as J\nimport qualified Data.Aeson as GuestAeson\nimport GHC.Generics (Generic)\n")
    replace_once(description,
        "data RedirectFileInfo = RedirectFileInfo\n  { size :: FileSize Int64,\n    digest :: FileDigest\n  }\n  deriving (Eq, Show)\n",
        "data RedirectFileInfo = RedirectFileInfo\n  { size :: FileSize Int64,\n    digest :: FileDigest\n  }\n  deriving (Eq, Show, Generic)\n")
    replace_once(description,
        "data YAMLFileDescription = YAMLFileDescription\n  { party :: FileParty,\n    size :: String,\n    digest :: FileDigest,\n    key :: C.SbKey,\n    nonce :: C.CbNonce,\n    chunkSize :: String,\n    replicas :: [YAMLServerReplicas],\n    redirect :: Maybe RedirectFileInfo\n  }\n  deriving (Eq, Show)\n",
        "data YAMLFileDescription = YAMLFileDescription\n  { party :: FileParty,\n    size :: String,\n    digest :: FileDigest,\n    key :: C.SbKey,\n    nonce :: C.CbNonce,\n    chunkSize :: String,\n    replicas :: [YAMLServerReplicas],\n    redirect :: Maybe RedirectFileInfo\n  }\n  deriving (Eq, Show, Generic)\n")
    replace_once(description,
        "data YAMLServerReplicas = YAMLServerReplicas\n  { server :: XFTPServer,\n    chunks :: [String]\n  }\n  deriving (Eq, Show)\n",
        "data YAMLServerReplicas = YAMLServerReplicas\n  { server :: XFTPServer,\n    chunks :: [String]\n  }\n  deriving (Eq, Show, Generic)\n")
    replace_once(description,
        "$(J.deriveJSON defaultJSON ''YAMLServerReplicas)\n\n$(J.deriveJSON defaultJSON ''RedirectFileInfo)\n\n$(J.deriveJSON defaultJSON ''YAMLFileDescription)",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "instance GuestAeson.ToJSON YAMLServerReplicas where\n"
        "  toJSON = GuestAeson.genericToJSON defaultJSON\n"
        "  toEncoding = GuestAeson.genericToEncoding defaultJSON\n"
        "instance GuestAeson.FromJSON YAMLServerReplicas where\n"
        "  parseJSON = GuestAeson.genericParseJSON defaultJSON\n"
        "instance GuestAeson.ToJSON RedirectFileInfo where\n"
        "  toJSON = GuestAeson.genericToJSON defaultJSON\n"
        "  toEncoding = GuestAeson.genericToEncoding defaultJSON\n"
        "instance GuestAeson.FromJSON RedirectFileInfo where\n"
        "  parseJSON = GuestAeson.genericParseJSON defaultJSON\n"
        "instance GuestAeson.ToJSON YAMLFileDescription where\n"
        "  toJSON = GuestAeson.genericToJSON defaultJSON\n"
        "  toEncoding = GuestAeson.genericToEncoding defaultJSON\n"
        "instance GuestAeson.FromJSON YAMLFileDescription where\n"
        "  parseJSON = GuestAeson.genericParseJSON defaultJSON\n"
        "#else\n"
        "$(J.deriveJSON defaultJSON ''YAMLServerReplicas)\n\n$(J.deriveJSON defaultJSON ''RedirectFileInfo)\n\n$(J.deriveJSON defaultJSON ''YAMLFileDescription)\n"
        "#endif")

    # These protocol dependencies have JSON instances generated with Aeson TH
    # upstream. Use the same Options through generic codecs in the guest build.
    remote_types = root / "Simplex/RemoteControl/Types.hs"
    replace_once(remote_types, "{-# LANGUAGE DataKinds #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DataKinds #-}\n{-# LANGUAGE DeriveGeneric #-}\n")
    replace_once(remote_types, "import qualified Data.Aeson as J\n",
                 "import qualified Data.Aeson as J\nimport GHC.Generics (Generic)\n")
    replace_once(remote_types,
        "  deriving (Eq, Show, Exception)\n",
        "  deriving (Eq, Show, Exception, Generic)\n\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "instance J.ToJSON RCErrorType where\n"
        "  toJSON = J.genericToJSON (sumTypeJSON $ dropPrefix \"RCE\")\n"
        "  toEncoding = J.genericToEncoding (sumTypeJSON $ dropPrefix \"RCE\")\n"
        "instance J.FromJSON RCErrorType where\n"
        "  parseJSON = J.genericParseJSON (sumTypeJSON $ dropPrefix \"RCE\")\n"
        "#endif\n")
    for splice in (
        "$(JQ.deriveJSON defaultJSON ''RCHostHello)",
        "$(JQ.deriveJSON defaultJSON {J.nullaryToObject = True} ''RCCtrlHello)",
        "$(JQ.deriveJSON (sumTypeJSON $ dropPrefix \"RCE\") ''RCErrorType)",
        "$(JQ.deriveJSON defaultJSON ''RCCtrlAddress)",
    ):
        if splice in remote_types.read_text():
            replace_once(remote_types, splice,
                         "#if !defined(SIMPLEX_MICROKIT)\n" + splice + "\n#endif")

    ratchet = base / "Crypto/Ratchet.hs"
    replace_once(ratchet, "{-# LANGUAGE DataKinds #-}\n",
                 "{-# LANGUAGE DataKinds #-}\n{-# LANGUAGE DeriveGeneric #-}\n")
    replace_once(ratchet, "import qualified Data.Aeson as J\n",
                 "import qualified Data.Aeson as J\nimport GHC.Generics (Generic)\n")
    for old, new in (
        ("  }\n  deriving (Show)\n\ndata RatchetVersions =", "  }\n  deriving (Show, Generic)\n\ndata RatchetVersions ="),
        ("  }\n  deriving (Show)\n\ndata RcvRatchet =", "  }\n  deriving (Show, Generic)\n\ndata RcvRatchet ="),
        ("  }\n  deriving (Show)\n\ndata RatchetKEM =", "  }\n  deriving (Show, Generic)\n\ndata RatchetKEM ="),
        ("  }\n  deriving (Show)\n\ndata RatchetKEMAccepted =", "  }\n  deriving (Show, Generic)\n\ndata RatchetKEMAccepted ="),
        ("  }\n  deriving (Show)\n\ntype SkippedMsgKeys =", "  }\n  deriving (Show, Generic)\n\ntype SkippedMsgKeys ="),
    ):
        replace_once(ratchet, old, new)
    replace_once(ratchet,
        "$(JQ.deriveJSON defaultJSON ''RcvRatchet)\n\n$(JQ.deriveJSON defaultJSON ''RatchetKEMAccepted)\n\n$(JQ.deriveJSON defaultJSON ''RatchetKEM)\n\ninstance AlgorithmI a => ToJSON (SndRatchet a) where\n  toEncoding = $(JQ.mkToEncoding defaultJSON ''SndRatchet)\n  toJSON = $(JQ.mkToJSON defaultJSON ''SndRatchet)\n\ninstance AlgorithmI a => FromJSON (SndRatchet a) where\n  parseJSON = $(JQ.mkParseJSON defaultJSON ''SndRatchet)\n\ninstance AlgorithmI a => ToJSON (Ratchet a) where\n  toEncoding = $(JQ.mkToEncoding defaultJSON ''Ratchet)\n  toJSON = $(JQ.mkToJSON defaultJSON ''Ratchet)\n\ninstance AlgorithmI a => FromJSON (Ratchet a) where\n  parseJSON = $(JQ.mkParseJSON defaultJSON ''Ratchet)",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "instance ToJSON RcvRatchet where\n  toJSON = J.genericToJSON defaultJSON\n  toEncoding = J.genericToEncoding defaultJSON\ninstance FromJSON RcvRatchet where\n  parseJSON = J.genericParseJSON defaultJSON\n"
        "instance ToJSON RatchetKEMAccepted where\n  toJSON = J.genericToJSON defaultJSON\n  toEncoding = J.genericToEncoding defaultJSON\ninstance FromJSON RatchetKEMAccepted where\n  parseJSON = J.genericParseJSON defaultJSON\n"
        "instance ToJSON RatchetKEM where\n  toJSON = J.genericToJSON defaultJSON\n  toEncoding = J.genericToEncoding defaultJSON\ninstance FromJSON RatchetKEM where\n  parseJSON = J.genericParseJSON defaultJSON\n"
        "instance AlgorithmI a => ToJSON (SndRatchet a) where\n  toEncoding = J.genericToEncoding defaultJSON\n  toJSON = J.genericToJSON defaultJSON\ninstance AlgorithmI a => FromJSON (SndRatchet a) where\n  parseJSON = J.genericParseJSON defaultJSON\n"
        "instance AlgorithmI a => ToJSON (Ratchet a) where\n  toEncoding = J.genericToEncoding defaultJSON\n  toJSON = J.genericToJSON defaultJSON\ninstance AlgorithmI a => FromJSON (Ratchet a) where\n  parseJSON = J.genericParseJSON defaultJSON\n"
        "#else\n"
        "$(JQ.deriveJSON defaultJSON ''RcvRatchet)\n\n$(JQ.deriveJSON defaultJSON ''RatchetKEMAccepted)\n\n$(JQ.deriveJSON defaultJSON ''RatchetKEM)\n\ninstance AlgorithmI a => ToJSON (SndRatchet a) where\n  toEncoding = $(JQ.mkToEncoding defaultJSON ''SndRatchet)\n  toJSON = $(JQ.mkToJSON defaultJSON ''SndRatchet)\n\ninstance AlgorithmI a => FromJSON (SndRatchet a) where\n  parseJSON = $(JQ.mkParseJSON defaultJSON ''SndRatchet)\n\ninstance AlgorithmI a => ToJSON (Ratchet a) where\n  toEncoding = $(JQ.mkToEncoding defaultJSON ''Ratchet)\n  toJSON = $(JQ.mkToJSON defaultJSON ''Ratchet)\n\ninstance AlgorithmI a => FromJSON (Ratchet a) where\n  parseJSON = $(JQ.mkParseJSON defaultJSON ''Ratchet)\n#endif")

    queue_info = base / "Server/QueueStore/QueueInfo.hs"
    replace_once(queue_info,
        "import Simplex.Messaging.Agent.Store.DB (FromField (..), ToField (..), fromTextField_)\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Simplex.Messaging.Agent.Store.DB (FromField (..), ToField (..), fromTextField_)\n"
        "#endif\n")
    wrap_once(queue_info,
        "$(JQ.deriveJSON (enumJSON $ dropPrefix \"Q\") ''QSubThread)\n\n"
        "$(JQ.deriveJSON defaultJSON ''QSub)\n\n"
        "$(JQ.deriveJSON (enumJSON $ dropPrefix \"MT\") ''MsgType)\n\n"
        "$(JQ.deriveJSON defaultJSON ''MsgInfo)\n\n"
        "$(JQ.deriveJSON defaultJSON ''QueueInfo)\n")
    replace_once(queue_info, "{-# LANGUAGE CPP #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DeriveGeneric #-}\n")
    replace_once(queue_info,
        "import Data.Time.Clock (UTCTime)\n",
        "import Data.Time.Clock (UTCTime)\nimport GHC.Generics (Generic)\n")
    replace_once(queue_info,
        "  deriving (Eq, Show)\n\n"
        "data QSub = QSub",
        "  deriving (Eq, Show, Generic)\n\n"
        "data QSub = QSub")
    for type_start in (
        "data QSub = QSub",
        "data QSubThread = QNoSub",
        "data MsgInfo = MsgInfo",
        "data MsgType = MTMessage",
        "data QueueMode = QMMessaging",
    ):
        segment = queue_info.read_text()
        offset = segment.index(type_start)
        end = segment.find("\n\n", offset)
        if end < 0:
            raise RuntimeError(f"unexpected QueueInfo declaration: {type_start}")
        declaration = segment[offset:end]
        if declaration.count("deriving (Eq, Show)") != 1:
            raise RuntimeError(f"unexpected QueueInfo deriving clause: {type_start}")
        queue_info.write_text(segment[:offset] + declaration.replace(
            "deriving (Eq, Show)", "deriving (Eq, Show, Generic)") + segment[end:])
    replace_once(queue_info,
        "instance Encoding QueueInfo where\n",
        """#if defined(SIMPLEX_MICROKIT)
instance J.ToJSON QSubThread where
  toJSON = J.genericToJSON (enumJSON $ dropPrefix "Q")
  toEncoding = J.genericToEncoding (enumJSON $ dropPrefix "Q")
instance J.FromJSON QSubThread where
  parseJSON = J.genericParseJSON (enumJSON $ dropPrefix "Q")
instance J.ToJSON QSub where
  toJSON = J.genericToJSON defaultJSON
  toEncoding = J.genericToEncoding defaultJSON
instance J.FromJSON QSub where
  parseJSON = J.genericParseJSON defaultJSON
instance J.ToJSON MsgType where
  toJSON = J.genericToJSON (enumJSON $ dropPrefix "MT")
  toEncoding = J.genericToEncoding (enumJSON $ dropPrefix "MT")
instance J.FromJSON MsgType where
  parseJSON = J.genericParseJSON (enumJSON $ dropPrefix "MT")
instance J.ToJSON MsgInfo where
  toJSON = J.genericToJSON defaultJSON
  toEncoding = J.genericToEncoding defaultJSON
instance J.FromJSON MsgInfo where
  parseJSON = J.genericParseJSON defaultJSON
instance J.ToJSON QueueInfo where
  toJSON = J.genericToJSON defaultJSON
  toEncoding = J.genericToEncoding defaultJSON
instance J.FromJSON QueueInfo where
  parseJSON = J.genericParseJSON defaultJSON
#endif

instance Encoding QueueInfo where
""")
    replace_once(queue_info,
        "instance FromField QueueMode where fromField = fromTextField_ $ eitherToMaybe . smpDecode . encodeUtf8\n\ninstance ToField QueueMode where toField = toField . decodeLatin1 . smpEncode\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "instance FromField QueueMode where fromField = fromTextField_ $ eitherToMaybe . smpDecode . encodeUtf8\n\n"
        "instance ToField QueueMode where toField = toField . decodeLatin1 . smpEncode\n"
        "#endif\n")
    service_scheme = base / "ServiceScheme.hs"
    replace_once(service_scheme, "{-# LANGUAGE LambdaCase #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE LambdaCase #-}\n")
    replace_once(service_scheme, "import Network.Socket (ServiceName)\n",
                 "#if !defined(SIMPLEX_MICROKIT)\nimport Network.Socket (ServiceName)\n#endif\n")
    replace_once(service_scheme,
        "import Simplex.Messaging.Transport.Client (TransportHost (..))\n",
        "import Simplex.Messaging.Transport.Client (TransportHost (..))\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "type ServiceName = String\n"
        "#endif\n")

    system_time = base / "SystemTime.hs"
    wrap_once(system_time, "import Simplex.Messaging.Agent.Store.DB (FromField (..), ToField (..))\n")
    replace_once(system_time,
        "  deriving newtype (FromJSON, ToJSON, FromField, ToField)\n",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "  deriving newtype (FromJSON, ToJSON)\n"
        "#else\n"
        "  deriving newtype (FromJSON, ToJSON, FromField, ToField)\n"
        "#endif\n")

    simplex_name = base / "SimplexName.hs"
    replace_once(simplex_name, "{-# LANGUAGE LambdaCase #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE LambdaCase #-}\n")
    wrap_once(simplex_name,
        "import Simplex.Messaging.Agent.Store.DB (FromField (..), ToField (..), fromTextField_)\n")
    wrap_once(simplex_name,
        "instance ToField SimplexDomain where toField = toField . decodeLatin1 . strEncode\n\n"
        "instance FromField SimplexDomain where fromField = fromTextField_ (eitherToMaybe . strDecode . encodeUtf8)\n")
    wrap_once(simplex_name,
        "$(J.deriveJSON (enumJSON $ dropPrefix \"TLD\") ''SimplexTLD)\n\n"
        "$(J.deriveJSON (enumJSON $ dropPrefix \"NT\") ''SimplexNameType)\n\n"
        "$(J.deriveJSON defaultJSON ''SimplexDomain)\n\n"
        "$(J.deriveJSON defaultJSON ''SimplexNameInfo)\n")

    name_record = base / "Names/Record.hs"
    replace_once(name_record, "{-# LANGUAGE NamedFieldPuns #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DeriveGeneric #-}\n{-# LANGUAGE NamedFieldPuns #-}\n")
    replace_once(name_record,
        "import Data.Text (Text)\n",
        "import Data.Text (Text)\nimport GHC.Generics (Generic)\n")
    replace_once(name_record,
        "  deriving (Eq, Show)\n\n-- omitNothingFields False so absent coin fields surface as JSON null (matches\n-- the resolver output for unset coins).\n$( JQ.deriveJSON\n    defaultJSON {J.omitNothingFields = False, J.fieldLabelModifier = dropPrefix \"nr\"}\n    ''NameRecord\n )\n",
        "  deriving (Eq, Show, Generic)\n\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "instance J.ToJSON NameRecord where\n"
        "  toJSON = J.genericToJSON nameRecordJSON\n"
        "  toEncoding = J.genericToEncoding nameRecordJSON\n"
        "instance J.FromJSON NameRecord where\n"
        "  parseJSON = J.genericParseJSON nameRecordJSON\n"
        "nameRecordJSON :: J.Options\n"
        "nameRecordJSON = defaultJSON {J.omitNothingFields = False, J.fieldLabelModifier = dropPrefix \"nr\"}\n"
        "#else\n"
        "-- omitNothingFields False so absent coin fields surface as JSON null (matches\n"
        "-- the resolver output for unset coins).\n$( JQ.deriveJSON\n    defaultJSON {J.omitNothingFields = False, J.fieldLabelModifier = dropPrefix \"nr\"}\n    ''NameRecord\n )\n"
        "#endif\n")

    sntrup = base / "Crypto/SNTRUP761/Bindings.hs"
    wrap_once(sntrup,
        "import Simplex.Messaging.Agent.Store.DB (FromField (..), ToField (..))\n")
    wrap_once(sntrup,
        "instance ToField KEMSharedKey where\n"
        "  toField (KEMSharedKey k) = toField (BA.convert k :: ByteString)\n\n"
        "instance FromField KEMSharedKey where\n"
        "#if defined(dbPostgres)\n"
        "  fromField f dat = KEMSharedKey_ . BA.convert @ByteString <$> fromField f dat\n"
        "#else\n"
        "  fromField f = KEMSharedKey_ . BA.convert @ByteString <$> fromField f\n"
        "#endif\n")

    ratchet = base / "Crypto/Ratchet.hs"
    wrap_once(ratchet,
        "import Simplex.Messaging.Agent.Store.DB (Binary (..), BoolInt (..), FromField (..), ToField (..), blobFieldDecoder)\n")
    for db_instances in (
        "instance ToField ARKEMParams where toField = toField . Binary . smpEncode\n\ninstance FromField ARKEMParams where fromField = blobFieldDecoder smpDecode\n",
        "instance RatchetKEMStateI s => ToField (PrivRKEMParams s) where toField = toField . Binary . smpEncode\n\ninstance (Typeable s, RatchetKEMStateI s) => FromField (PrivRKEMParams s) where fromField = blobFieldDecoder smpDecode\n",
        "instance ToField MessageKey where toField = toField . Binary . smpEncode\n\ninstance FromField MessageKey where fromField = blobFieldDecoder smpDecode\n",
        "instance AlgorithmI a => ToField (Ratchet a) where toField = toField . Binary . LB.toStrict . J.encode\n\ninstance (AlgorithmI a, Typeable a) => FromField (Ratchet a) where fromField = blobFieldDecoder J.eitherDecodeStrict'\n\ninstance ToField PQEncryption where toField (PQEncryption pqEnc) = toField (BI pqEnc)\n\ninstance FromField PQEncryption where\n#if defined(dbPostgres)\n  fromField f dat = PQEncryption . unBI <$> fromField f dat\n#else\n  fromField f = PQEncryption . unBI <$> fromField f\n#endif\n\ninstance ToField PQSupport where toField (PQSupport pqEnc) = toField (BI pqEnc)\n\ninstance FromField PQSupport where\n#if defined(dbPostgres)\n  fromField f dat = PQSupport . unBI <$> fromField f dat\n#else\n  fromField f = PQSupport . unBI <$> fromField f\n#endif\n",
        "instance AlgorithmI a => ToField (MsgEncryptKey a) where toField = toField . Binary . smpEncode\n\ninstance (AlgorithmI a, Typeable a) => FromField (MsgEncryptKey a) where fromField = blobFieldDecoder smpDecode\n",
    ):
        wrap_once(ratchet, db_instances)

    sqlite_db = base / "Agent/Store/SQLite/DB.hs"
    replace_once(sqlite_db, "{-# LANGUAGE DeriveAnyClass #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DeriveAnyClass #-}\n")
    wrap_once(sqlite_db, "{-# LANGUAGE TemplateHaskell #-}\n")
    wrap_once(sqlite_db, "import qualified Data.Aeson.TH as J\n")
    wrap_once(sqlite_db, "$(J.deriveJSON defaultJSON ''SlowQueryStats)\n")

    # The target GHC is built without a code interpreter, so SQLite's SQL
    # quasiquoter cannot run during compilation. The migration files contain
    # only static SQL; lower those quotations to the equivalent Query bytes
    # in the staged guest tree. The ordinary host sources remain untouched.
    migration_dir = base / "Agent/Store/SQLite/Migrations"
    for migration in migration_dir.glob("*.hs"):
        source = migration.read_text()
        quote_count = source.count("[sql|")
        if not quote_count:
            continue
        if source.count("{-# LANGUAGE QuasiQuotes #-}") != 1:
            raise RuntimeError(f"unexpected SQL migration language pragmas: {migration}")
        source = source.replace("{-# LANGUAGE QuasiQuotes #-}\n", "{-# LANGUAGE CPP #-}\n")
        if source.count("import Database.SQLite.Simple.QQ (sql)\n") != 1:
            raise RuntimeError(f"unexpected SQL migration quasiquoter import: {migration}")
        source = source.replace(
            "import Database.SQLite.Simple.QQ (sql)\n",
            "import qualified Data.Text as GuestSQL\n",
        )
        source = source.replace(
            "import Database.SQLite.Simple (Query)\n",
            "import Database.SQLite.Simple (Query (..))\n",
        )
        source, replacements = re.subn(
            r"\[sql\|(.*?)\|\]",
            lambda match: "(Query (GuestSQL.pack " + json.dumps(match.group(1)) + "))",
            source,
            flags=re.DOTALL,
        )
        if replacements != quote_count:
            raise RuntimeError(
                f"expected {quote_count} SQL quotations in {migration}; converted {replacements}"
            )
        migration.write_text(source)

    shared = base / "Agent/Store/Shared.hs"
    replace_once(shared, "{-# LANGUAGE LambdaCase #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE LambdaCase #-}\n")
    wrap_once(shared, "{-# LANGUAGE TemplateHaskell #-}\n")
    wrap_once(shared, "import qualified Data.Aeson.TH as J\n")
    for splice in (
        "$(J.deriveJSON (sumTypeJSON $ dropPrefix \"MTRE\") ''MTRError)\n",
        "$(J.deriveJSON defaultJSON ''UpMigration)\n",
        "$(J.deriveToJSON (sumTypeJSON $ dropPrefix \"ME\") ''MigrationError)\n",
    ):
        wrap_once(shared, splice)

    # The native messaging agent is an HTTP/2 client. Its common body helper
    # also carries an instance for the upstream HTTP/2 server Request type,
    # which pulls the server/socket stack into this client-only dependency
    # closure. Keep that instance for host builds, but omit it in the guest;
    # the server-side module is not used by the phone client.
    http2 = base / "Transport/HTTP2.hs"
    replace_once(http2, "{-# LANGUAGE NamedFieldPuns #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE NamedFieldPuns #-}\n")
    replace_once(http2,
        "import qualified Network.HTTP2.Server as HS\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import qualified Network.HTTP2.Server as HS\n"
        "#endif\n",
    )
    replace_once(http2,
        "instance HTTP2BodyChunk HS.Request where\n"
        "  getBodyChunk = HS.getRequestBodyChunk\n"
        "  {-# INLINE getBodyChunk #-}\n"
        "  getBodySize = HS.requestBodySize\n"
        "  {-# INLINE getBodySize #-}\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "instance HTTP2BodyChunk HS.Request where\n"
        "  getBodyChunk = HS.getRequestBodyChunk\n"
        "  {-# INLINE getBodyChunk #-}\n"
        "  getBodySize = HS.requestBodySize\n"
        "  {-# INLINE getBodySize #-}\n"
        "#endif\n",
    )

    # SimpleX's agent and XFTP client use Network.Socket.HostName only as a
    # string alias. Fence that host package import out of the guest dependency
    # graph; actual name resolution and connection opening remain explicit
    # network-PD capabilities, not fake socket implementations.
    agent_client = base / "Agent/Client.hs"
    replace_once(agent_client,
        "import Network.Socket (HostName)\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Network.Socket (HostName)\n"
        "#endif\n",
    )
    replace_once(agent_client,
        "type ClientVar msg = SessionVar (Either (AgentErrorType, Maybe UTCTime) (Client msg))\n",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "type HostName = String\n"
        "#endif\n\n"
        "type ClientVar msg = SessionVar (Either (AgentErrorType, Maybe UTCTime) (Client msg))\n",
    )
    file_transfer_client = root / "Simplex/FileTransfer/Client.hs"
    replace_once(file_transfer_client, "{-# LANGUAGE BlockArguments #-}\n",
                 "{-# LANGUAGE CPP #-}\n{-# LANGUAGE BlockArguments #-}\n")
    replace_once(file_transfer_client,
        "import Network.Socket (HostName)\n",
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Network.Socket (HostName)\n"
        "#endif\n",
    )
    replace_once(file_transfer_client,
        "data XFTPClient = XFTPClient\n",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "type HostName = String\n"
        "#endif\n\n"
        "data XFTPClient = XFTPClient\n",
    )


if __name__ == "__main__":
    import sys
    prepare(Path(sys.argv[1]))
