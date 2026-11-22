#!/usr/bin/env python3
"""Typecheck upstream Agent.Client against the current Microkit guest shims."""

import subprocess
import re
import json
import sys
from pathlib import Path

from prepare_simplexmq_guest_transport import prepare
from verify_simplexmq_backend_patch import FULL_STAGE, ROOT, ensure_patch_applied, stage_full_source


def share_agent_file_error_types() -> None:
    """Share upstream file-transfer error types in the integrated client build."""
    protocol = FULL_STAGE / "Simplex/Messaging/Agent/Protocol.hs"
    text = protocol.read_text()
    marker = "-- Protocol-facing XFTP errors, copied from FileTransfer.Transport so\n"
    end_marker = "-- SMP agent protocol version history:\n"
    start = text.find(marker)
    end = text.find(end_marker, start)
    if start < 0 or end < 0 or text.count(marker) != 1:
        raise RuntimeError("Agent.Protocol target file-transfer schema block changed")
    text = text[:start] + text[end:]
    text = text.replace("    XFTPErrorType (..),\n", "    XFTP.XFTPErrorType,\n", 1)
    text = text.replace(
        "    FILE {fileErr :: FileErrorType}\n",
        "    FILE {fileErr :: FT.FileErrorType}\n",
        1,
    )
    text = text.replace("xftpErr :: XFTPErrorType", "xftpErr :: XFTP.XFTPErrorType", 1)
    anchor = "import Simplex.Messaging.Version.Internal\n"
    if text.count(anchor) != 1:
        raise RuntimeError("Agent.Protocol import anchor changed")
    text = text.replace(
        anchor,
        anchor
        + "import qualified Simplex.FileTransfer.Transport as XFTP\n"
        + "import qualified Simplex.FileTransfer.Types as FT\n",
        1,
    )
    protocol.write_text(text)


def prepare_remote_control_discovery() -> None:
    """Fence LAN discovery/listeners from the client-only Microkit build."""
    remote_types = FULL_STAGE / "Simplex/RemoteControl/Types.hs"
    types_text = remote_types.read_text()
    types_text = types_text.replace(
        "  deriving (Show)\n\n#if !defined(SIMPLEX_MICROKIT)\n$(JQ.deriveJSON defaultJSON ''RCHostHello)",
        "  deriving (Show, Generic)\n\n#if defined(SIMPLEX_MICROKIT)\n"
        "instance J.ToJSON RCHostHello where\n"
        "  toJSON = J.genericToJSON defaultJSON\n"
        "  toEncoding = J.genericToEncoding defaultJSON\n"
        "instance J.FromJSON RCHostHello where parseJSON = J.genericParseJSON defaultJSON\n"
        "#else\n$(JQ.deriveJSON defaultJSON ''RCHostHello)",
        1,
    )
    types_text = types_text.replace(
        "data RCCtrlHello = RCCtrlHello {}\n  deriving (Show)\n\n#if !defined(SIMPLEX_MICROKIT)\n$(JQ.deriveJSON defaultJSON {J.nullaryToObject = True} ''RCCtrlHello)",
        "data RCCtrlHello = RCCtrlHello {}\n  deriving (Show, Generic)\n\n#if defined(SIMPLEX_MICROKIT)\n"
        "instance J.ToJSON RCCtrlHello where\n"
        "  toJSON = J.genericToJSON (defaultJSON {J.nullaryToObject = True})\n"
        "  toEncoding = J.genericToEncoding (defaultJSON {J.nullaryToObject = True})\n"
        "instance J.FromJSON RCCtrlHello where\n"
        "  parseJSON = J.genericParseJSON (defaultJSON {J.nullaryToObject = True})\n"
        "#else\n$(JQ.deriveJSON defaultJSON {J.nullaryToObject = True} ''RCCtrlHello)",
        1,
    )
    remote_types.write_text(types_text)

    client = FULL_STAGE / "Simplex/RemoteControl/Client.hs"
    text = client.read_text()
    if "{-# LANGUAGE CPP #-}" not in text:
        text = "{-# LANGUAGE CPP #-}\n" + text
    text = text.replace(
        "import Control.Applicative ((<|>))\n",
        "import Control.Applicative ((<|>))\n"
        "import Control.Concurrent.Async (Async)\n"
        "import UnliftIO.Async (async, uninterruptibleCancel)\n",
        1,
    )
    text = text.replace(
        "import Network.Socket (PortNumber, SockAddr (..), hostAddressToTuple)\n",
        "import Network.Socket (PortNumber)\n"
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Network.Socket (SockAddr (..), hostAddressToTuple)\n"
        "#endif\n",
        1,
    )
    text = text.replace("import qualified Network.UDP as UDP\n", "")
    text = text.replace(
        "import Network.Socket (SockAddr (..), hostAddressToTuple)\n#endif\n",
        "import Network.Socket (SockAddr (..), hostAddressToTuple)\n"
        "import qualified Network.UDP as UDP\n#endif\n",
        1,
    )
    transport_import = (
        "import Simplex.Messaging.Transport.Client (TransportClientConfig (..), TransportHost (..), "
        "defaultTransportClientConfig, runTransportClient)\n"
    )
    if text.count(transport_import) != 1:
        raise RuntimeError("RemoteControl.Client transport import changed")
    text = text.replace(
        transport_import,
        "import Simplex.Messaging.Transport.Client (TransportHost (..))\n"
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Simplex.Messaging.Transport.Client (TransportClientConfig (..), defaultTransportClientConfig, runTransportClient)\n"
        "#endif\n",
        1,
    )
    announce = "announceRC drg maxCount idPrivKey knownDhPub RCHostKeys {sessKeys, dhKeys} inv = ExceptT $ withSender $ \\sender -> runExceptT $ do\n"
    if text.count(announce) != 1:
        raise RuntimeError("RemoteControl.Client announceRC insertion point changed")
    text = text.replace(
        announce,
        "announceRC drg maxCount idPrivKey knownDhPub RCHostKeys {sessKeys, dhKeys} inv =\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "  throwE RCENotDiscovered\n"
        "#else\n"
        "  ExceptT $ withSender $ \\sender -> runExceptT $ do\n",
        1,
    )
    marker = "    (dhPubKey, dhPrivKey) = dhKeys\n\n"
    if text.count(marker) != 1:
        raise RuntimeError("RemoteControl.Client announceRC end marker changed")
    text = text.replace(marker, marker + "#endif\n\n", 1)
    discover = "discoverRCCtrl subscribers pairings =\n"
    if text.count(discover) != 1:
        raise RuntimeError("RemoteControl.Client discoverRCCtrl insertion point changed")
    text = text.replace(
        discover,
        discover
        + "#if defined(SIMPLEX_MICROKIT)\n"
        + "  throwE RCENotDiscovered\n"
        + "#else\n"
        + "  timeoutThrow RCENotDiscovered 30000000 $ ExceptT $ withListener subscribers $ \\listener ->\n",
        1,
    )
    marker = "      pure r\n  where\n"
    if text.count(marker) != 1:
        raise RuntimeError("RemoteControl.Client discoverRCCtrl end marker changed")
    text = text.replace(marker, "      pure r\n  where\n", 1)
    marker = "\nfindRCCtrlPairing ::"
    if text.count(marker) != 1:
        raise RuntimeError("RemoteControl.Client discovery loop end marker changed")
    text = text.replace(
        marker,
        "\n#endif\n\nfindRCCtrlPairing ::",
        1,
    )
    run_client = "    runClient RCCClient_ {confirmSession, endSession} r = do\n"
    if text.count(run_client) != 1:
        raise RuntimeError("RemoteControl.Client controller runClient insertion point changed")
    text = text.replace(
        run_client,
        "    runClient RCCClient_ {confirmSession, endSession} r =\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "      throwE RCENotDiscovered\n"
        "#else\n"
        "      do\n",
        1,
    )
    marker = "            logDebug \"Session ended\"\n"
    if text.count(marker) != 1:
        raise RuntimeError("RemoteControl.Client controller runClient end marker changed")
    text = text.replace(marker, marker + "#endif\n", 1)
    client.write_text(text)

    # The guest target package set has Data.Composition's operators only up to
    # four arguments, while this pinned upstream Agent module uses its five-
    # argument variant. Keep this exact adapter local to the staged module.
    agent = FULL_STAGE / "Simplex/Messaging/Agent.hs"
    agent_text = agent.read_text()
    agent_text = agent_text.replace(
        "import UnliftIO.Async (mapConcurrently)\n",
        "import UnliftIO.Async (mapConcurrently)\n",
        1,
    )
    agent_text = agent_text.replace(
        "    runAgent = do\n",
        "    runAgent :: ReaderT Env IO AgentClient\n"
        "    runAgent = do\n",
        1,
    )
    agent_text = agent_text.replace(
        "    runAgentThreads c\n",
        "    runAgentThreads :: AgentClient -> ReaderT Env IO ()\n"
        "    runAgentThreads c\n",
        1,
    )
    anchor = "type AE a = ExceptT AgentErrorType IO a\n"
    if agent_text.count(anchor) != 1:
        raise RuntimeError("Agent.hs type alias insertion point changed")
    agent_text = agent_text.replace(
        anchor,
        "infixr 9 .::.\n"
        "(.::.) :: (m r -> n r) -> (a -> b -> c -> d -> e -> m r) -> a -> b -> c -> d -> e -> n r\n"
        "f .::. g = \\a b c d e -> f (g a b c d e)\n\n"
        + anchor,
        1,
    )
    agent.write_text(agent_text)


def prepare_sqlite_guest_source() -> None:
    source = FULL_STAGE / "Simplex/Messaging/Agent/Store/SQLite.hs"
    text = source.read_text()
    text = text.replace("{-# LANGUAGE QuasiQuotes #-}\n", "", 1)
    text = text.replace("import Database.SQLite.Simple.QQ (sql)\n", "", 1)
    pattern = re.compile(r"(?ms)      SQLite3\.exec db' \. fromQuery \$\n        \[sql\|.*?\|\]")
    replacement = (
        "      SQLite3.exec db' $\n"
        '        "PRAGMA busy_timeout = 100; " <>\n'
        '        "PRAGMA foreign_keys = ON; " <>\n'
        '        "PRAGMA secure_delete = ON; " <>\n'
        '        "PRAGMA auto_vacuum = FULL;"'
    )
    text, count = pattern.subn(replacement, text)
    if count != 1:
        raise RuntimeError("expected exactly one static SQLite PRAGMA quasiquote")
    source.write_text(text)

    # The upstream SQLite transaction helper can mask the first SQL failure
    # when its cleanup ROLLBACK also fails. Keep the guest's parameterized SQL
    # statement and original exception in the QEMU trace to diagnose VFS faults.
    source = FULL_STAGE / "Simplex/Messaging/Agent/Store/SQLite/DB.hs"
    text = source.read_text()
    anchor = """      a `catch` \\e -> do
        atomically $ TM.alter (Just . updateQueryErrors e) sql slow
        throwIO e"""
    replacement = """      a `catch` \\e -> do
#ifdef SIMPLEX_MICROKIT
        putStrLn (\"SIMPLEX_SQL_ERROR|\" <> show sql <> \"|\" <> show e)
#endif
        atomically $ TM.alter (Just . updateQueryErrors e) sql slow
        throwIO e"""
    if text.count(anchor) != 1:
        raise RuntimeError("expected exactly one SQLite query exception handler")
    source.write_text(text.replace(anchor, replacement, 1))

    # AgentStore's SQL quasiquotes are all static statements with ordinary
    # SQLite '?' parameters. Lower them to Query Text while keeping the
    # prepared-statement binding path unchanged.
    source = FULL_STAGE / "Simplex/Messaging/Agent/Store/AgentStore.hs"
    text = source.read_text()
    quote_count = text.count("[sql|")
    if quote_count != 128:
        raise RuntimeError(f"expected 128 static AgentStore SQL quotations, found {quote_count}")
    text = text.replace("{-# LANGUAGE QuasiQuotes #-}\n", "{-# LANGUAGE CPP #-}\n", 1)
    text = text.replace("import Database.SQLite.Simple.QQ (sql)\n", "import qualified Data.Text as GuestSQL\n", 1)
    text, replacements = re.subn(
        r"\[sql\|(.*?)\|]",
        lambda match: "(Query (GuestSQL.pack " + json.dumps(match.group(1)) + "))",
        text,
        flags=re.DOTALL,
    )
    if replacements != quote_count:
        raise RuntimeError(f"converted {replacements} of {quote_count} AgentStore SQL quotations")
    source.write_text(text)


def prepare_agent_json() -> None:
    """Replace only the remaining client-closure Aeson TH splices."""
    base = FULL_STAGE / "Simplex/Messaging"

    def add_guest_prelude(path: Path) -> str:
        text = path.read_text()
        if "{-# LANGUAGE CPP #-}" not in text:
            text = "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DeriveGeneric #-}\n" + text
        if "import qualified Data.Aeson as GuestAeson\n" not in text:
            if "import Data.Aeson (" in text:
                text = text.replace("import Data.Aeson (", "import qualified Data.Aeson as GuestAeson\nimport Data.Aeson (", 1)
            else:
                text = text.replace("import Control.Exception\n", "import Control.Exception\nimport qualified Data.Aeson as GuestAeson\n", 1)
        if "import GHC.Generics (Generic)\n" not in text:
            # Insert beside a stable import in each source file.
            anchor = "import Control.Exception\n"
            if anchor not in text:
                anchor = "import Data.Int (Int64)\n"
            if anchor not in text:
                raise RuntimeError(f"no import insertion point for {path}")
            text = text.replace(anchor, anchor + "import GHC.Generics (Generic)\n", 1)
        return text

    stats = base / "Agent/Stats.hs"
    text = add_guest_prelude(stats)
    for type_name in ("AgentSMPServerStatsData", "AgentXFTPServerStatsData",
                      "AgentNtfServerStatsData", "AgentPersistedServerStats"):
        splice = f"$(J.deriveJSON defaultJSON ''{type_name})"
        generic = (
            f"deriving stock instance Generic {type_name}\n"
            f"instance ToJSON {type_name} where\n"
            f"  toJSON = GuestAeson.genericToJSON defaultJSON\n"
            f"  toEncoding = GuestAeson.genericToEncoding defaultJSON\n"
            f"instance FromJSON {type_name} where\n"
            f"  parseJSON = GuestAeson.genericParseJSON defaultJSON"
        )
        if text.count(splice) != 1:
            raise RuntimeError(f"expected one Agent.Stats JSON splice for {type_name}")
        text = text.replace(splice, f"#if defined(SIMPLEX_MICROKIT)\n{generic}\n#else\n{splice}\n#endif")
    stats.write_text(text)

    crypto_file = base / "Crypto/File.hs"
    text = add_guest_prelude(crypto_file)
    for type_name in ("CryptoFileArgs", "CryptoFile"):
        splice = f"$(J.deriveJSON defaultJSON ''{type_name})"
        generic = (
            f"deriving stock instance Generic {type_name}\n"
            f"instance ToJSON {type_name} where\n"
            f"  toJSON = GuestAeson.genericToJSON defaultJSON\n"
            f"  toEncoding = GuestAeson.genericToEncoding defaultJSON\n"
            f"instance FromJSON {type_name} where\n"
            f"  parseJSON = GuestAeson.genericParseJSON defaultJSON"
        )
        if text.count(splice) != 1:
            raise RuntimeError(f"expected one Crypto.File JSON splice for {type_name}")
        text = text.replace(splice, f"#if defined(SIMPLEX_MICROKIT)\n{generic}\n#else\n{splice}\n#endif")
    # The module's explicit Aeson imports are otherwise limited to TH.
    text = text.replace("import qualified Data.Aeson.TH as J\n", "#if !defined(SIMPLEX_MICROKIT)\nimport qualified Data.Aeson.TH as J\n#endif\n", 1)
    if "import Data.Aeson (FromJSON (..), ToJSON (..))\n" not in text:
        text = text.replace("import Control.Exception\n", "import Control.Exception\nimport Data.Aeson (FromJSON (..), ToJSON (..))\n", 1)
    text = text.replace("tag' <- LC.sbAuth <$> readTVarIO sb", "tag' <- liftIO $ LC.sbAuth <$> readTVarIO sb", 1)
    crypto_file.write_text(text)

    env = base / "Agent/Env/SQLite.hs"
    text = add_guest_prelude(env)
    text = text.replace("$(pure [])\n\n$(JQ.deriveJSON defaultJSON ''ServerRoles)",
        "#if !defined(SIMPLEX_MICROKIT)\n$(pure [])\n#endif\n\n#if defined(SIMPLEX_MICROKIT)\n"
        "deriving stock instance Generic ServerRoles\n"
        "instance ToJSON ServerRoles where\n"
        "  toJSON = GuestAeson.genericToJSON defaultJSON\n"
        "  toEncoding = GuestAeson.genericToEncoding defaultJSON\n"
        "instance FromJSON ServerRoles where\n"
        "  parseJSON = GuestAeson.genericParseJSON defaultJSON\n"
        "#else\n$(JQ.deriveJSON defaultJSON ''ServerRoles)\n#endif")
    server_cfg_instances = re.compile(
        r"(?ms)^instance ProtocolTypeI p => ToJSON \(ServerCfg p\) where\n.*?"
        r"^instance ProtocolTypeI p => FromJSON \(ServerCfg p\) where\n.*?(?=\n\S|\Z)"
    )
    replacement = (
        "#if defined(SIMPLEX_MICROKIT)\n"
        "deriving stock instance Generic (ServerCfg p)\n"
        "#endif\n"
        "instance ProtocolTypeI p => ToJSON (ServerCfg p) where\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "  toEncoding = GuestAeson.genericToEncoding defaultJSON\n"
        "  toJSON = GuestAeson.genericToJSON defaultJSON\n"
        "#else\n"
        "  toEncoding = $(JQ.mkToEncoding defaultJSON ''ServerCfg)\n"
        "  toJSON = $(JQ.mkToJSON defaultJSON ''ServerCfg)\n"
        "#endif\n\n"
        "instance ProtocolTypeI p => FromJSON (ServerCfg p) where\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "  parseJSON = GuestAeson.genericParseJSON defaultJSON\n"
        "#else\n"
        "  parseJSON = $(JQ.mkParseJSON defaultJSON ''ServerCfg)\n"
        "#endif"
    )
    text, n = server_cfg_instances.subn(replacement, text)
    if n != 1:
        raise RuntimeError("expected the two ServerCfg JSON instance declarations")
    text = text.replace("import qualified Data.Aeson.TH as JQ\n", "#if !defined(SIMPLEX_MICROKIT)\nimport qualified Data.Aeson.TH as JQ\n#endif\n", 1)
    if text == env.read_text() or "deriving stock instance Generic ServerRoles" not in text:
        raise RuntimeError("failed to stage generic Agent.Env.SQLite JSON instances")
    env.write_text(text)

    # The protocol-only build keeps a local ProxyClientError to avoid pulling
    # in the complete client. The integrated client closure already includes
    # Messaging.Client, so share its type here to keep error values identical.
    agent_protocol = base / "Agent/Protocol.hs"
    text = agent_protocol.read_text()
    proxy_copy = re.compile(
        r"(?ms)^-- Target-only copy of Client\.ProxyClientError\..*?"
        r"^instance FromJSON ProxyClientError where\n"
        r"  parseJSON = GuestAeson\.genericParseJSON \(sumTypeJSON \$ dropPrefix \"Proxy\"\)\n\n"
    )
    text, n = proxy_copy.subn("", text)
    if n != 1:
        raise RuntimeError("expected target-local Agent.Protocol ProxyClientError shim")
    anchor = "import Simplex.Messaging.Agent.QueryString\n"
    if text.count(anchor) != 1:
        raise RuntimeError("Agent.Protocol import anchor changed")
    text = text.replace(anchor,
        "import Simplex.Messaging.Agent.QueryString\n"
        "import Simplex.Messaging.Client (ProxyClientError (..))\n", 1)
    agent_protocol.write_text(text)

    agent = base / "Agent/Client.hs"
    text = agent.read_text()
    text = text.replace("{-# LANGUAGE ConstraintKinds #-}\n", "{-# LANGUAGE DeriveGeneric #-}\n{-# LANGUAGE ConstraintKinds #-}\n", 1)
    if "import GHC.Generics (Generic)\n" not in text:
        text = text.replace("import qualified Data.Aeson as J\n", "import qualified Data.Aeson as J\nimport GHC.Generics (Generic)\n", 1)
    if "import Data.Aeson (FromJSON (..), ToJSON (..))\n" not in text:
        text = text.replace("import qualified Data.Aeson as J\n",
                            "import qualified Data.Aeson as J\nimport Data.Aeson (FromJSON (..), ToJSON (..))\n", 1)
    if "import qualified Simplex.Messaging.Agent.Protocol as GuestAgentProtocol\n" not in text:
        text = text.replace("import Simplex.Messaging.Agent.Protocol\n",
                            "import Simplex.Messaging.Agent.Protocol\n"
                            "import qualified Simplex.Messaging.Agent.Protocol as GuestAgentProtocol\n", 1)
    if "import NativeTLSBackend (nativeTLSBackendConnector)\n" not in text:
        text = text.replace("import Control.Applicative ((<|>))\n",
                            "import Control.Applicative ((<|>))\n"
                            "import NativeTLSBackend (nativeTLSBackendConnector)\n", 1)
    text = text.replace("import Data.List (find, foldl')\n",
                        "import Data.List (find, foldl', intercalate)\n", 1)
    connector_helper = (
        "nativeAgentTLSConnector host service =\n"
        "  nativeTLSBackendConnector (case host of\n"
        "    THDomainName domain -> domain\n"
        "    THIPv4 (a, b, c, d) -> intercalate \".\" (map show [a, b, c, d])\n"
        "    THIPv6 _ -> error \"Microkit native transport supports IPv4 only\"\n"
        "    THOnionHost _ -> error \"Microkit native transport does not support onion hosts\") service\n\n"
    )
    client_start = "getSMPServerClient :: AgentClient -> NetworkRequestMode -> SMPTransportSession -> AM SMPConnectedClient\n"
    if text.count(client_start) != 1:
        raise RuntimeError("Agent.Client SMP client insertion point changed")
    if "nativeAgentTLSConnector host service" not in text:
        text = text.replace(client_start, connector_helper + client_start, 1)
    native_calls = text.count("getProtocolClient g nm")
    if native_calls != 4:
        raise RuntimeError(f"expected 4 Agent.Client protocol connection sites, got {native_calls}")
    text = text.replace("getProtocolClient g nm", "getProtocolClientWithTLSBackend nativeAgentTLSConnector g nm")
    text = text.replace("    clientSocksCredentials,\n",
                        "#if defined(SIMPLEX_MICROKIT)\n    SocksCredentials (..),\n#endif\n"
                        "    clientSocksCredentials,\n", 1)
    text = text.replace("import qualified Data.Aeson.TH as J\n", "#if !defined(SIMPLEX_MICROKIT)\nimport qualified Data.Aeson.TH as J\n#endif\n", 1)
    mappings = (
        ("AgentLocks", "defaultJSON"),
        ("ProtocolTestStep", '(enumJSON $ dropPrefix "TS")'),
        ("ProtocolTestFailure", "defaultJSON"),
        ("ServerSessions", "defaultJSON"),
        ("SMPServerSubs", "defaultJSON"),
        ("AgentServersSummary", "defaultJSON"),
        ("SubInfo", "defaultJSON"),
        ("SubscriptionsInfo", "defaultJSON"),
        ("WorkersDetails", "defaultJSON"),
        ("WorkersSummary", "defaultJSON"),
        ("AgentWorkersDetails", 'defaultJSON {J.fieldLabelModifier = takeWhile (/= \'_\')}'),
        ("AgentWorkersSummary", "defaultJSON"),
        ("ClientInfo", '(sumTypeJSON $ dropPrefix "ClientInfo")'),
        ("AgentQueuesInfo", "defaultJSON"),
        ("UserNetworkType", '(enumJSON $ dropPrefix "UN")'),
        ("UserNetworkInfo", "defaultJSON"),
        ("ServerQueueInfo", "defaultJSON"),
    )
    for type_name, options in mappings:
        pattern = re.compile(rf"\$\(J\.deriveJSON\s+{re.escape(options)}\s+''{type_name}\)")
        match = pattern.search(text)
        if not match:
            raise RuntimeError(f"missing Agent.Client Aeson splice for {type_name}")
        generic = (
            f"deriving stock instance Generic {type_name}\n"
            f"instance ToJSON {type_name} where\n"
            f"  toJSON = J.genericToJSON ({options})\n"
            f"  toEncoding = J.genericToEncoding ({options})\n"
            f"instance FromJSON {type_name} where\n"
            f"  parseJSON = J.genericParseJSON ({options})"
        )
        splice = match.group(0)
        text = text.replace(splice, f"#if defined(SIMPLEX_MICROKIT)\n{generic}\n#else\n{splice}\n#endif", 1)
    agent.write_text(text)

    # Agent.Protocol shares the upstream XFTP type in this integrated closure,
    # so preserve type identity instead of converting target-local copies.
    agent = base / "Agent/Client.hs"
    text = agent.read_text()
    xftp_instance = "instance ProtocolServerClient XFTPVersion XFTPErrorType FileResponse where"
    converter = (
        "xftpErrorToAgentProtocol :: XFTP.XFTPErrorType -> GuestAgentProtocol.XFTPErrorType\n"
        "xftpErrorToAgentProtocol = id\n\n"
        "xftpProtocolError :: HostName -> XFTP.XFTPErrorType -> AgentErrorType\n"
        "xftpProtocolError host = XFTP host . xftpErrorToAgentProtocol\n\n"
    )
    if text.count(xftp_instance) != 1:
        raise RuntimeError("expected a single XFTP ProtocolServerClient instance")
    text = text.replace(xftp_instance,
                        converter + "instance ProtocolServerClient XFTPVersion XFTP.XFTPErrorType FileResponse where", 1)
    text = text.replace("  clientProtocolError = XFTP\n  closeProtocolServerClient = X.closeXFTPClient",
                        "  clientProtocolError host err = XFTP host (xftpErrorToAgentProtocol err)\n"
                        "  closeProtocolServerClient = X.closeXFTPClient", 1)
    text = text.replace("$ XFTP (B.unpack $ strEncode srv) DIGEST",
                        "$ XFTP (B.unpack $ strEncode srv) XFTP.DIGEST", 1)
    text = text.replace("  XFTP _ XFTP.TIMEOUT -> True",
                        "  XFTP _ XFTP.TIMEOUT -> True", 1)
    text = text.replace("protocolClientError XFTP $ B.unpack $ strEncode srv",
                        "protocolClientError xftpProtocolError $ B.unpack $ strEncode srv")
    text = text.replace("protocolClientError XFTP addr",
                        "protocolClientError xftpProtocolError addr")
    agent.write_text(text)

    messaging_client = base / "Client.hs"
    text = messaging_client.read_text()
    if "{-# LANGUAGE DeriveGeneric #-}" not in text:
        text = "{-# LANGUAGE DeriveGeneric #-}\n" + text
    if "import qualified Data.Aeson as GuestAeson\n" not in text:
        text = text.replace("import qualified Data.Aeson.TH as J\n",
                            "import qualified Data.Aeson as GuestAeson\n"
                            "import qualified Data.Aeson.TH as J\n", 1)
    if "import GHC.Generics (Generic)\n" not in text:
        text = text.replace("import Control.Applicative ((<|>))\n",
                            "import Control.Applicative ((<|>))\nimport GHC.Generics (Generic)\n", 1)
    marker = "    clientSocksCredentials,\n"
    if text.count(marker) != 1:
        raise RuntimeError("Messaging.Client credential export point changed")
    text = text.replace(marker,
        "#if defined(SIMPLEX_MICROKIT)\n    SocksCredentials (..),\n#endif\n" + marker, 1)
    proxy_splice = "$(J.deriveJSON (sumTypeJSON $ dropPrefix \"Proxy\") ''ProxyClientError)"
    client_json_guard = "#if !defined(SIMPLEX_MICROKIT)\n$(J.deriveJSON (enumJSON $ dropPrefix \"HM\") ''HostMode)"
    proxy_generic = (
        "#if defined(SIMPLEX_MICROKIT)\n"
        "deriving stock instance Generic ProxyClientError\n"
        "instance GuestAeson.ToJSON ProxyClientError where\n"
        "  toJSON = GuestAeson.genericToJSON (sumTypeJSON $ dropPrefix \"Proxy\")\n"
        "  toEncoding = GuestAeson.genericToEncoding (sumTypeJSON $ dropPrefix \"Proxy\")\n"
        "instance GuestAeson.FromJSON ProxyClientError where\n"
        "  parseJSON = GuestAeson.genericParseJSON (sumTypeJSON $ dropPrefix \"Proxy\")\n"
        "#endif\n\n"
    )
    queue_info_generic = (
        "#if defined(SIMPLEX_MICROKIT)\n"
        "deriving stock instance Generic TBQueueInfo\n"
        "instance GuestAeson.ToJSON TBQueueInfo where\n"
        "  toJSON = GuestAeson.genericToJSON defaultJSON\n"
        "  toEncoding = GuestAeson.genericToEncoding defaultJSON\n"
        "instance GuestAeson.FromJSON TBQueueInfo where\n"
        "  parseJSON = GuestAeson.genericParseJSON defaultJSON\n"
        "#endif\n\n"
    )
    if text.count(proxy_splice) != 1 or text.count(client_json_guard) != 1:
        raise RuntimeError("Messaging.Client JSON staging anchors changed")
    text = text.replace(client_json_guard, proxy_generic + queue_info_generic + client_json_guard, 1)
    messaging_client.write_text(text)

    messaging_agent = base / "Agent.hs"
    text = messaging_agent.read_text()
    text = text.replace("{-# LANGUAGE ConstraintKinds #-}\n", "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DeriveGeneric #-}\n{-# LANGUAGE ConstraintKinds #-}\n", 1)
    text = text.replace("import qualified Data.Aeson.TH as JQ\n", "#if !defined(SIMPLEX_MICROKIT)\nimport qualified Data.Aeson.TH as JQ\n#endif\n", 1)
    if "import GHC.Generics (Generic)\n" not in text:
        text = text.replace("import Data.Aeson (FromJSON (..), ToJSON (..))\n", "import Data.Aeson (FromJSON (..), ToJSON (..))\nimport GHC.Generics (Generic)\n", 1)
    replacement = (
        "#if !defined(SIMPLEX_MICROKIT)\n$(pure [])\n#endif\n\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "deriving stock instance Generic (DatabaseDiff a)\n"
        "instance FromJSON a => FromJSON (DatabaseDiff a) where\n"
        "  parseJSON = J.genericParseJSON defaultJSON\n"
        "instance ToJSON a => ToJSON (DatabaseDiff a) where\n"
        "  toEncoding = J.genericToEncoding defaultJSON\n"
        "  toJSON = J.genericToJSON defaultJSON\n"
        "#else\n$(pure [])\n"
        "instance FromJSON a => FromJSON (DatabaseDiff a) where\n"
        "  parseJSON = $(JQ.mkParseJSON defaultJSON ''DatabaseDiff)\n\n"
        "instance ToJSON a => ToJSON (DatabaseDiff a) where\n"
        "  toEncoding = $(JQ.mkToEncoding defaultJSON ''DatabaseDiff)\n"
        "  toJSON = $(JQ.mkToJSON defaultJSON ''DatabaseDiff)\n"
        "#endif"
    )
    pattern = re.compile(r"(?ms)^\$\(pure \[\]\)\n\ninstance FromJSON a => FromJSON \(DatabaseDiff a\) where\n.*?\$\(JQ\.mkToJSON defaultJSON ''DatabaseDiff\)")
    text, n = pattern.subn(replacement, text)
    if n != 1:
        raise RuntimeError("expected the DatabaseDiff TH instance block")
    messaging_agent.write_text(text)

    for relative, type_name in (
        ("../FileTransfer/Types.hs", "FileErrorType"),
        ("../FileTransfer/Transport.hs", "XFTPErrorType"),
    ):
        source = (base / relative).resolve()
        text = source.read_text()
        text = text.replace("{-# LANGUAGE DuplicateRecordFields #-}\n",
                            "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DeriveGeneric #-}\n{-# LANGUAGE DuplicateRecordFields #-}\n", 1)
        text = text.replace("import qualified Data.Aeson.TH as J\n",
                            "#if !defined(SIMPLEX_MICROKIT)\nimport qualified Data.Aeson.TH as J\n#endif\n"
                            "import Data.Aeson (FromJSON (..), ToJSON (..))\n"
                            "import qualified Data.Aeson as GuestAeson\nimport GHC.Generics (Generic)\n", 1)
        splice = f"$(J.deriveJSON (sumTypeJSON id) ''{type_name})"
        generic = (
            f"deriving stock instance Generic {type_name}\n"
            f"instance ToJSON {type_name} where\n"
            "  toJSON = GuestAeson.genericToJSON (sumTypeJSON id)\n"
            "  toEncoding = GuestAeson.genericToEncoding (sumTypeJSON id)\n"
            f"instance FromJSON {type_name} where\n"
            "  parseJSON = GuestAeson.genericParseJSON (sumTypeJSON id)"
        )
        if text.count(splice) != 1:
            raise RuntimeError(f"expected one file-transfer JSON splice for {type_name}")
        text = text.replace(splice,
                            f"#if defined(SIMPLEX_MICROKIT)\n{generic}\n#else\n{splice}\n#endif", 1)
        source.write_text(text)

    # The upstream XFTP protocol imports a narrow shim to break the protocol /
    # transport cycle on host builds. The staged guest transport is now the
    # authoritative provider, so share its type identities instead of
    # accidentally compiling a second XFTPVersion universe.
    protocol = (base / "../FileTransfer/Protocol.hs").resolve()
    text = protocol.read_text()
    old = "import Simplex.FileTransfer.Transport.Types (XFTPErrorType (..), XFTPVersion, blockedFilesXFTPVersion, xftpClientHandshakeStub)"
    new = "import Simplex.FileTransfer.Transport (XFTPErrorType (..), XFTPVersion, blockedFilesXFTPVersion, xftpClientHandshakeStub)"
    if text.count(old) != 1:
        raise RuntimeError("XFTP guest protocol type import changed")
    protocol.write_text(text.replace(old, new, 1))


def prepare_http2_backend() -> None:
    """Give the guest HTTP/2 client an explicit network-PD backend hook."""
    source = FULL_STAGE / "Simplex/Messaging/Transport/HTTP2/Client.hs"
    text = source.read_text()
    text = text.replace("{-# LANGUAGE DataKinds #-}\n", "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DataKinds #-}\n", 1)
    old_import = "import Simplex.Messaging.Transport.Client (TransportClientConfig (..), TransportHost (..), defaultTcpConnectTimeout, runTLSTransportClient)\n"
    new_import = (
        "#if defined(SIMPLEX_MICROKIT)\n"
        "import Simplex.Messaging.Transport.Client (TransportClientConfig (..), TransportHost (..), defaultTcpConnectTimeout, runTLSBackendTransportClient)\n"
        "#else\n" + old_import + "#endif\n"
    )
    if text.count(old_import) != 1:
        raise RuntimeError("HTTP/2 TLS client import changed")
    text = text.replace(old_import, new_import, 1)
    socks_import = "import Network.Socks5 (SocksCredentials)\n"
    socks_replacement = (
        "#if defined(SIMPLEX_MICROKIT)\n"
        "import Simplex.Messaging.Client (SocksCredentials)\n"
        "#else\n" + socks_import + "#endif\n"
    )
    if text.count(socks_import) != 1:
        raise RuntimeError("HTTP/2 SOCKS credentials import changed")
    text = text.replace(socks_import, socks_replacement, 1)
    text = text.replace("    suportedTLSParams :: T.Supported\n  }\n  deriving (Show)",
        "    suportedTLSParams :: T.Supported\n#if defined(SIMPLEX_MICROKIT)\n    , networkBackend :: Maybe (TransportHost -> ServiceName -> IO T.Backend)\n#endif\n  }\n#if defined(SIMPLEX_MICROKIT)\ninstance Show HTTP2ClientConfig where show _ = \"HTTP2ClientConfig{guest backend}\"\n#else\n  deriving (Show)\n#endif", 1)
    text = text.replace("      suportedTLSParams = http2TLSParams\n    }", "      suportedTLSParams = http2TLSParams\n#if defined(SIMPLEX_MICROKIT)\n      , networkBackend = Nothing\n#endif\n    }", 1)
    text = text.replace("    setup = runHTTP2Client (suportedTLSParams config) caStore (transportConfig config) (bufferSize config) socksCreds host port keyHash",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "    setup = runHTTP2Client (networkBackend config) (suportedTLSParams config) caStore (transportConfig config) (bufferSize config) socksCreds host port keyHash\n"
        "#else\n"
        "    setup = runHTTP2Client (suportedTLSParams config) caStore (transportConfig config) (bufferSize config) socksCreds host port keyHash\n"
        "#endif", 1)
    text = text.replace("runHTTP2Client :: forall a. T.Supported -> Maybe XS.CertificateStore -> TransportClientConfig -> BufferSize -> Maybe SocksCredentials -> TransportHost -> ServiceName -> Maybe C.KeyHash -> (TLS 'TClient -> H.Client a) -> IO a\nrunHTTP2Client tlsParams caStore tcConfig bufferSize socksCreds host port keyHash = runHTTP2ClientWith bufferSize host setup\n  where\n    setup :: (TLS 'TClient -> IO a) -> IO a\n    setup = runTLSTransportClient tlsParams caStore tcConfig socksCreds host port keyHash",
        "#if defined(SIMPLEX_MICROKIT)\n"
        "runHTTP2Client :: forall a. Maybe (TransportHost -> ServiceName -> IO T.Backend) -> T.Supported -> Maybe XS.CertificateStore -> TransportClientConfig -> BufferSize -> Maybe SocksCredentials -> TransportHost -> ServiceName -> Maybe C.KeyHash -> (TLS 'TClient -> H.Client a) -> IO a\n"
        "runHTTP2Client backend tlsParams caStore tcConfig bufferSize socksCreds host port keyHash = runHTTP2ClientWith bufferSize host setup\n"
        "  where\n"
        "    setup :: (TLS 'TClient -> IO a) -> IO a\n"
        "    setup client = case backend of\n"
        "      Nothing -> E.throwIO $ userError \"Microkit network backend capability not installed\"\n"
        "      Just openBackend -> runTLSBackendTransportClient tlsParams caStore tcConfig host port keyHash (openBackend host port) client\n"
        "#else\n"
        "runHTTP2Client :: forall a. T.Supported -> Maybe XS.CertificateStore -> TransportClientConfig -> BufferSize -> Maybe SocksCredentials -> TransportHost -> ServiceName -> Maybe C.KeyHash -> (TLS 'TClient -> H.Client a) -> IO a\nrunHTTP2Client tlsParams caStore tcConfig bufferSize socksCreds host port keyHash = runHTTP2ClientWith bufferSize host setup\n"
        "  where\n"
        "    setup :: (TLS 'TClient -> IO a) -> IO a\n"
        "    setup = runTLSTransportClient tlsParams caStore tcConfig socksCreds host port keyHash\n"
        "#endif", 1)
    if "runTLSBackendTransportClient tlsParams" not in text:
        raise RuntimeError("failed to stage explicit HTTP/2 network backend")
    source.write_text(text)


def prepare_xftp_decrypt_ttl_skew() -> None:
    """Keep XFTP rcv/snd work items visible despite guest clock skew."""
    source = FULL_STAGE / "Simplex/Messaging/Agent/Store/AgentStore.hs"
    text = source.read_text()
    # After GuestSQL conversion these three workers still use -ttl.
    targets = (
        "SELECT rcv_file_id",
        "SELECT snd_file_id",
        "SELECT r.snd_file_chunk_replica_id, f.snd_file_id",
    )
    changed = 0
    for marker in targets:
        idx = text.find(marker)
        if idx < 0:
            continue
        window = text[max(0, idx - 220):idx]
        if "ttl * 10000" in window:
            continue
        old = "cutoffTs <- addUTCTime (-ttl) <$> getCurrentTime"
        pos = text.rfind(old, 0, idx)
        if pos < 0:
            continue
        text = (
            text[:pos]
            + "cutoffTs <- addUTCTime (-(ttl * 10000)) <$> getCurrentTime"
            + text[pos + len(old):]
        )
        changed += 1
    if changed == 0:
        # SOURCE form may already include the factor from tree patches.
        if text.count("ttl * 10000") < 3:
            raise RuntimeError("XFTP snd/rcv TTL skew cutoffs not found")
    # Skip upload rows whose prefix_path was cleared after a failed prepare;
    # bare FilePath decoding of NULL aborts the snd worker with ConversionFailed.
    # GuestSQL conversion keeps newlines as literal \n inside the pack string.
    if "AND f.failed = 0 AND f.prefix_path IS NOT NULL" not in text:
        old_filter = r"AND f.failed = 0\n            ORDER BY r.retries ASC, r.created_at ASC"
        new_filter = (
            r"AND f.failed = 0 AND f.prefix_path IS NOT NULL\n"
            r"            ORDER BY r.retries ASC, r.created_at ASC"
        )
        if old_filter not in text:
            raise RuntimeError("snd chunk upload failed-filter not found")
        text = text.replace(old_filter, new_filter, 1)
    if "Maybe FilePath) :. (Int64, Int, Int64, Word32, FileDigest)" not in text:
        old_sig = (
            "toChunk :: ((DBSndFileId, SndFileId, UserId, Int, FilePath) :. "
            "(Int64, Int, Int64, Word32, FileDigest) :. "
            "(Int64, ChunkReplicaId, C.APrivateAuthKey, SndFileReplicaStatus, Maybe Int64, Int)) "
            "-> SndFileChunk\n"
            "        toChunk ((sndFileId, sndFileEntityId, userId, numRecipients, filePrefixPath) :. "
            "(sndChunkId, chunkNo, chunkOffset, chunkSize, digest) :. "
            "(sndChunkReplicaId, replicaId, replicaKey, replicaStatus, delay, retries)) =\n"
            "          let chunkSpec = XFTPChunkSpec {filePath = sndFileEncPath filePrefixPath, chunkOffset, chunkSize}\n"
            "           in SndFileChunk"
        )
        new_sig = (
            "toChunk :: ((DBSndFileId, SndFileId, UserId, Int, Maybe FilePath) :. "
            "(Int64, Int, Int64, Word32, FileDigest) :. "
            "(Int64, ChunkReplicaId, C.APrivateAuthKey, SndFileReplicaStatus, Maybe Int64, Int)) "
            "-> Either StoreError SndFileChunk\n"
            "        toChunk ((sndFileId, sndFileEntityId, userId, numRecipients, Just filePrefixPath) :. "
            "(sndChunkId, chunkNo, chunkOffset, chunkSize, digest) :. "
            "(sndChunkReplicaId, replicaId, replicaKey, replicaStatus, delay, retries)) =\n"
            "          let chunkSpec = XFTPChunkSpec {filePath = sndFileEncPath filePrefixPath, chunkOffset, chunkSize}\n"
            "           in Right SndFileChunk"
        )
        if old_sig not in text:
            raise RuntimeError("snd chunk toChunk signature not found for prefix_path null fix")
        text = text.replace(old_sig, new_sig, 1)
        text = text.replace(
            "firstRow toChunk SEFileNotFound $\n"
            "          DB.query\n"
            "            db\n"
            "            (Query (GuestSQL.pack \"\\n              SELECT\\n                f.snd_file_id, f.snd_file_entity_id, f.user_id, f.num_recipients, f.prefix_path,",
            "firstRow' toChunk SEFileNotFound $\n"
            "          DB.query\n"
            "            db\n"
            "            (Query (GuestSQL.pack \"\\n              SELECT\\n                f.snd_file_id, f.snd_file_entity_id, f.user_id, f.num_recipients, f.prefix_path,",
            1,
        )
        # Close the Just-pattern with a Nothing branch before the next top-level decl.
        anchor = "replicas = [SndFileChunkReplica {sndChunkReplicaId, server, replicaId, replicaKey, replicaStatus, delay, retries, rcvIdsKeys = []}]\n                }"
        if anchor not in text:
            raise RuntimeError("snd chunk toChunk body anchor not found")
        text = text.replace(
            anchor,
            anchor + "\n        toChunk _ = Left SEFileNotFound",
            1,
        )
    source.write_text(text)



def prepare_xftp_snd_worker_logs() -> None:
    """Trace snd prepare past encrypt so a silent hang is locatable."""
    source = FULL_STAGE / "Simplex/FileTransfer/Agent.hs"
    text = source.read_text()
    if "XFTP_SND_PREPARE|ENCRYPT_BEGIN" in text:
        return
    old = (
        "            withStore' c $ \\db -> updateSndFileStatus db sndFileId SFSEncrypting\n"
        "            (digest, chunkSpecsDigests) <- encryptFileForUpload sndFile fsEncPath\n"
    )
    new = (
        "            withStore' c $ \\db -> updateSndFileStatus db sndFileId SFSEncrypting\n"
        "            liftIO $ putStrLn \"XFTP_SND_PREPARE|ENCRYPT_BEGIN\"\n"
        "            (digest, chunkSpecsDigests) <- encryptFileForUpload sndFile fsEncPath\n"
        "            liftIO $ putStrLn \"XFTP_SND_PREPARE|ENCRYPT_DONE\"\n"
    )
    if old not in text:
        raise RuntimeError("snd prepare encrypt site not found")
    text = text.replace(old, new, 1)
    old2 = "      srvs <- forM pendingChunks $ createChunk numRecipients'\n"
    new2 = (
        "      liftIO $ putStrLn (\"XFTP_SND_PREPARE|CREATE_CHUNKS|\" <> show (length pendingChunks))\n"
        "      srvs <- forM pendingChunks $ createChunk numRecipients'\n"
        "      liftIO $ putStrLn \"XFTP_SND_PREPARE|CREATE_CHUNKS_DONE\"\n"
    )
    if old2 not in text:
        raise RuntimeError("snd prepare createChunk site not found")
    text = text.replace(old2, new2, 1)
    source.write_text(text)


def prepare_xftp_http2_backend() -> None:
    """Wire NativeTLS into XFTP HTTP/2 clients (default config leaves backend Nothing)."""
    source = FULL_STAGE / "Simplex/FileTransfer/Client.hs"
    text = source.read_text()
    if "{-# LANGUAGE CPP #-}" not in "\n".join(text.splitlines()[:12]):
        text = text.replace(
            "{-# LANGUAGE DataKinds #-}\n",
            "{-# LANGUAGE CPP #-}\n{-# LANGUAGE DataKinds #-}\n",
            1,
        )
    if "nativeTLSBackendEphemeralConnector" not in text:
        text = text.replace(
            "import NativeTLSBackend (nativeTLSBackendConnector)\n",
            "import NativeTLSBackend (nativeTLSBackendEphemeralConnector)\n",
            1,
        )
    if "import NativeTLSBackend" not in text:
        text = text.replace(
            "import Control.Monad\n",
            "import Control.Monad\n"
            "#if defined(SIMPLEX_MICROKIT)\n"
            "import Data.List (intercalate)\n"
            "import NativeTLSBackend (nativeTLSBackendEphemeralConnector)\n"
            "#endif\n",
            1,
        )
    text = text.replace(
        "import Simplex.Messaging.Transport.Client (TransportClientConfig (..), TransportHost)\n",
        "import Simplex.Messaging.Transport.Client (TransportClientConfig (..), TransportHost (..))\n",
        1,
    )
    if "nativeTLSBackendConnector\n          (case host of" in text:
        text = text.replace(
            "nativeTLSBackendConnector\n          (case host of",
            "nativeTLSBackendEphemeralConnector\n          (case host of",
            1,
        )
    if "networkBackend = Just" in text and "nativeTLSBackendEphemeralConnector" in text:
        source.write_text(text)
        return
    old = (
        "xftpHTTP2Config :: TransportClientConfig -> XFTPClientConfig -> HTTP2ClientConfig\n"
        "xftpHTTP2Config transportConfig XFTPClientConfig {xftpNetworkConfig = NetworkConfig {tcpConnectTimeout}} =\n"
        "  defaultHTTP2ClientConfig\n"
        "    { bodyHeadSize = xftpBlockSize,\n"
        "      suportedTLSParams = defaultSupportedParams,\n"
        "      connTimeout = netTimeoutInt tcpConnectTimeout NRMBackground,\n"
        "      transportConfig\n"
        "    }"
    )
    new = (
        "xftpHTTP2Config :: TransportClientConfig -> XFTPClientConfig -> HTTP2ClientConfig\n"
        "xftpHTTP2Config transportConfig XFTPClientConfig {xftpNetworkConfig = NetworkConfig {tcpConnectTimeout}} =\n"
        "  defaultHTTP2ClientConfig\n"
        "    { bodyHeadSize = xftpBlockSize,\n"
        "      suportedTLSParams = defaultSupportedParams,\n"
        "      connTimeout = netTimeoutInt tcpConnectTimeout NRMBackground,\n"
        "      transportConfig\n"
        "#if defined(SIMPLEX_MICROKIT)\n"
        "    , networkBackend = Just $ \\host service ->\n"
        "        nativeTLSBackendEphemeralConnector\n"
        "          (case host of\n"
        "             THDomainName domain -> domain\n"
        "             THIPv4 (a, b, c, d) -> intercalate \".\" (map show [a, b, c, d])\n"
        "             THIPv6 _ -> error \"Microkit XFTP transport supports IPv4 only\"\n"
        "             THOnionHost _ -> error \"Microkit XFTP transport does not support onion hosts\")\n"
        "          service\n"
        "#endif\n"
        "    }"
    )
    if "nativeTLSBackendEphemeralConnector" in text and "networkBackend = Just" in text:
        source.write_text(text)
        return
    if old not in text:
        # Already wired with the old connector name — rewrite in place.
        if "nativeTLSBackendConnector\n          (case host of" in text:
            text = text.replace(
                "nativeTLSBackendConnector\n          (case host of",
                "nativeTLSBackendEphemeralConnector\n          (case host of",
                1,
            )
            source.write_text(text)
            return
        raise RuntimeError("xftpHTTP2Config block not found for NativeTLS wiring")
    source.write_text(text.replace(old, new, 1))


def prepare_guest_store_codecs() -> None:
    """Enable upstream SQLite codecs in the compile-only guest source tree."""
    messaging = FULL_STAGE / "Simplex/Messaging"

    crypto = messaging / "Crypto.hs"
    text = crypto.read_text()
    text = text.replace("#if !defined(SIMPLEX_MICROKIT)\n", "#if defined(SIMPLEX_MICROKIT)\n")
    if "import Simplex.Messaging.Agent.Store.DB (Binary (..), FromField (..), ToField (..), blobFieldDecoder)" not in text:
        raise RuntimeError("Crypto DB codec import was not enabled for guest source")
    crypto.write_text(text)

    ratchet = messaging / "Crypto/Ratchet.hs"
    text = ratchet.read_text().replace("#if !defined(SIMPLEX_MICROKIT)\n", "#if defined(SIMPLEX_MICROKIT)\n")
    ratchet.write_text(text)

    sntrup = messaging / "Crypto/SNTRUP761/Bindings.hs"
    text = sntrup.read_text().replace("#if !defined(SIMPLEX_MICROKIT)\n", "#if defined(SIMPLEX_MICROKIT)\n")
    sntrup.write_text(text)

    protocol = messaging / "Protocol.hs"
    text = protocol.read_text()
    db_import = (
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Simplex.Messaging.Agent.Store.DB (Binary (..), FromField (..), ToField (..))\n"
        "#endif\n"
    )
    if text.count(db_import) != 1:
        raise RuntimeError("Protocol DB codec import guard changed")
    text = text.replace(db_import, "import Simplex.Messaging.Agent.Store.DB (Binary (..), FromField (..), ToField (..))\n", 1)
    text = text.replace("#if !defined(SIMPLEX_MICROKIT)\ninstance ToField EncDataBytes where",
                        "#if defined(SIMPLEX_MICROKIT)\ninstance ToField EncDataBytes where", 1)
    text = text.replace("#if defined(SIMPLEX_MICROKIT)\n  deriving newtype (StrEncoding)\n#else\n  deriving newtype (FromField, StrEncoding)",
                        "#if defined(SIMPLEX_MICROKIT)\n  deriving newtype (FromField, StrEncoding)\n#else\n  deriving newtype (FromField, StrEncoding)", 1)
    text = text.replace("deriving newtype (Encoding)\n#else\n  deriving newtype (Encoding, FromField)\n\ninstance ToField IdsHash",
                        "deriving newtype (Encoding, FromField)\n"
                        "instance ToField IdsHash where toField (IdsHash s) = toField (Binary s)\n"
                        "#else\n  deriving newtype (Encoding, FromField)\n\ninstance ToField IdsHash", 1)
    protocol.write_text(text)

    queue_info = messaging / "Server/QueueStore/QueueInfo.hs"
    text = queue_info.read_text()
    db_import = (
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "import Simplex.Messaging.Agent.Store.DB (FromField (..), ToField (..), fromTextField_)\n"
        "#endif\n"
    )
    if text.count(db_import) != 1:
        raise RuntimeError("QueueInfo DB codec import guard changed")
    text = text.replace(db_import,
        "import Simplex.Messaging.Agent.Store.DB (FromField (..), ToField (..), fromTextField_)\n", 1)
    db_instance = (
        "#if !defined(SIMPLEX_MICROKIT)\n"
        "instance FromField QueueMode where fromField = fromTextField_ $ eitherToMaybe . smpDecode . encodeUtf8\n\n"
        "instance ToField QueueMode where toField = toField . decodeLatin1 . smpEncode\n"
        "#endif\n"
    )
    if text.count(db_instance) != 1:
        raise RuntimeError("QueueInfo QueueMode DB codec guard changed")
    text = text.replace(db_instance,
        "instance FromField QueueMode where fromField = fromTextField_ $ eitherToMaybe . smpDecode . encodeUtf8\n\n"
        "instance ToField QueueMode where toField = toField . decodeLatin1 . smpEncode\n", 1)
    queue_info.write_text(text)

    system_time = messaging / "SystemTime.hs"
    text = system_time.read_text()
    text = text.replace("#if !defined(SIMPLEX_MICROKIT)\nimport Simplex.Messaging.Agent.Store.DB (FromField (..), ToField (..))\n#endif\n",
                        "import Simplex.Messaging.Agent.Store.DB (FromField (..), ToField (..))\n", 1)
    text = text.replace("#if defined(SIMPLEX_MICROKIT)\n  deriving newtype (FromJSON, ToJSON)",
                        "#if defined(SIMPLEX_MICROKIT)\n  deriving newtype (FromJSON, ToJSON, FromField, ToField)", 1)
    system_time.write_text(text)



def main() -> None:
    build_objects = "--objects" in sys.argv[1:]
    ensure_patch_applied()
    stage_full_source()
    prepare(FULL_STAGE)
    share_agent_file_error_types()
    prepare_remote_control_discovery()
    prepare_sqlite_guest_source()
    prepare_agent_json()
    prepare_http2_backend()
    prepare_xftp_http2_backend()
    prepare_xftp_decrypt_ttl_skew()
    prepare_xftp_snd_worker_logs()
    prepare_guest_store_codecs()
    ghc_tree = ROOT / ".cache/ghc-port/ghc-9.6.6"
    source = ROOT / ".cache/guest-http2-microkit/http2-4.2.2"
    compiler = ghc_tree / "_build/stage0/bin/aarch64-unknown-none-ghc"
    target_db = ghc_tree / "_build/stage1/lib/package.conf.d"
    guest_db = ROOT / ".cache/sel4-tls-deps/guest-package.conf.d"
    autogen = (ROOT / ".cache/simplex-chat-upstream/dist-newstyle/build/aarch64-osx"
               / "ghc-9.6.3/simplexmq-7.0.0.6/build/autogen")
    output = ROOT / "experiments/ghc-microkit/build/guest-simplex-agent-client-check"
    output.mkdir(parents=True, exist_ok=True)
    paths = [
        ROOT / "experiments/ghc-microkit",
        ROOT / "experiments/ghc-microkit/guest-transport-shims",
        ROOT / "experiments/ghc-microkit/guest-client-shims",
        ROOT / "experiments/ghc-microkit/guest-http2-shims",
        source,
        FULL_STAGE,
        autogen,
    ]
    ghc_mode = ["--make", "-no-link"] if build_objects else ["-fno-code"]
    args = [str(compiler), *ghc_mode, "-fforce-recomp", "-v0",
            "-no-global-package-db", "-package-env=-",
        "-package-db", str(target_db), "-package-db", str(guest_db),
        "-DSIMPLEX_MICROKIT", "-XStrictData", "-XOverloadedStrings", "-XDerivingStrategies",
        *[f"-i{path}" for path in paths],
        "-optc-target", "-optcaarch64-none-elf",
        f"-optc-I{ROOT / 'experiments/ghc-microkit/freestanding-headers'}",
        f"-optc-I{ghc_tree / 'rts/include'}", f"-optc-I{ghc_tree / 'rts'}",
        "-outputdir", str(output),
            str(FULL_STAGE / "Simplex/Messaging/Agent/Client.hs")]
    result = subprocess.run(args, cwd=ghc_tree, capture_output=True, text=True)
    if result.returncode:
        diagnostics = output / ("object-build.stderr" if build_objects else "typecheck.stderr")
        diagnostics.write_text(result.stderr)
        raise RuntimeError(
            ("TARGET AGENT CLIENT|OBJECT BUILD BLOCKED\n" if build_objects
             else "TARGET AGENT CLIENT|TYPECHECK BLOCKED\n")
            + f"Full compiler diagnostics: {diagnostics}\n"
            + result.stderr[:12000]
        )
    if build_objects:
        objects = list(output.rglob("*.o"))
        print(f"TARGET AGENT CLIENT|AARCH64 OBJECT BUILD PASS|{len(objects)} objects")
    else:
        print("TARGET AGENT CLIENT|TYPECHECK PASS")


if __name__ == "__main__":
    main()
