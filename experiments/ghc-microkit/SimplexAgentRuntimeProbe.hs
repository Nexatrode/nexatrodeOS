{-# LANGUAGE DataKinds #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE PatternSynonyms #-}
{-# LANGUAGE GADTs #-}
{-# LANGUAGE ScopedTypeVariables #-}

-- | Run an upstream encrypted invitation/message loop against local SMP.
module Main (main) where

import qualified Data.ByteArray as BA
import Data.ByteArray (ScrubbedBytes)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BSC
import qualified Data.ByteString.Lazy as BL
import qualified Data.Aeson as Aeson
import Data.Aeson ((.=), (.:), (.:?), (.!=), FromJSON (..), withObject)
import Data.Aeson.Types (Parser)
import qualified Data.Text.Encoding as TE
import qualified Data.Text as Text
import Control.Concurrent.STM (atomically, readTBQueue, tryReadTBQueue)
#if defined(SIMPLEX_AGENT_SERVICE_MODE) || defined(SIMPLEX_PUBLIC_SMP)
import Data.Int (Int64)
import Data.Word (Word8, Word16, Word32, Word64)
#endif
#ifdef SIMPLEX_AGENT_SERVICE_MODE
import Control.Concurrent (threadDelay)
import Data.Bits (xor, shiftL, shiftR, (.|.), (.&.))
import Foreign.C.Types (CInt (..))
import Foreign.Marshal.Alloc (alloca, allocaBytes)
import Foreign.Ptr (Ptr, castPtr, nullPtr)
import Foreign.Storable (peek)
#endif
import Control.Monad (forM_, void, when)
import Data.String (fromString)
import Data.List (find, foldl')
import Data.List.NonEmpty (NonEmpty (..))
import Data.IORef (IORef, atomicModifyIORef', modifyIORef', newIORef, readIORef, writeIORef)
import qualified Data.Map.Strict as M
import Control.Monad.Except (ExceptT, catchError, runExceptT, throwError)
import Control.Monad.IO.Class (liftIO)
import qualified Simplex.Messaging.Agent as Agent
import qualified Simplex.Messaging.Agent.Protocol as AgentProtocol
import Simplex.Messaging.Agent.Env.SQLite
  ( InitialAgentServers (..),
    createAgentStore,
    defaultAgentConfig,
    allRoles,
    presetServerCfg,
  )
import Simplex.Messaging.Agent.Store.AgentStore as AgentStore
import Simplex.Messaging.Agent.Store (AcceptedConfirmation (senderConf))
import Simplex.Messaging.Agent.Store.SQLite (closeDBStore)
import Simplex.Messaging.Agent.Store.SQLite.Common (DBOpts (..), DBStore, withTransaction, withTransaction')
import qualified Database.SQLite.Simple as SQL
import Simplex.Messaging.Agent.Store.SQLite.DB (TrackQueries (TQOff))
import Simplex.Messaging.Agent.Store.Shared
  ( MigrationConfig (..),
    MigrationConfirmation (MCYesUpDown),
  )
#ifdef SIMPLEX_PUBLIC_SMP
import qualified Data.Sequence as Seq
import Crypto.Random.Entropy (getEntropy)
import NativeNetwork (tryAcquireEphemeralGate)
import Simplex.FileTransfer.Client.Presets (defaultXFTPServers)
import Simplex.FileTransfer.Description
  ( FileChunk (..),
    FileChunkReplica (..),
    FileDescription (..),
    ValidFileDescription,
    pattern ValidFileDescription,
  )
import Simplex.FileTransfer.Protocol (FileParty (..))
import Simplex.Messaging.Client (HostMode (HMPublic), NetworkConfig (hostMode), defaultNetworkConfig, pattern NRMInteractive)
import Simplex.Messaging.Crypto.File (plain)
import System.Directory (createDirectoryIfMissing, removeFile)
import qualified Control.Exception as Exception
import System.IO.Unsafe (unsafePerformIO)
#else
import Simplex.Messaging.Client (defaultNetworkConfig, pattern NRMInteractive)
import System.IO.Unsafe (unsafePerformIO)
#endif
import Simplex.Messaging.Crypto (KeyHash, certificateFingerprint)
import Simplex.Messaging.Crypto.Ratchet (pattern IKPQOff, pattern IKPQOn, pattern PQEncOn, pattern PQSupportOn)
import Simplex.Messaging.Encoding.String (StrEncoding (strDecode, strEncode))
import Simplex.Messaging.Protocol (SubscriptionMode (..), noAuthSrv, noMsgFlags, pattern SMPServer)
import Simplex.Messaging.Transport.Client (TransportHost (..))
import qualified Data.X509 as X509
import System.Timeout (timeout)
import System.IO (BufferMode (LineBuffering), hSetBuffering, stdout)
#ifdef SIMPLEX_AGENT_SERVICE_MODE
import Control.Monad (forever)
#endif
import SMPFixtureCA (caDER)

#ifdef SIMPLEX_PUBLIC_SMP
data PendingXftpSend = PendingXftpSend
  { pendingXftpSndId :: B.ByteString
  , pendingXftpConn :: AgentProtocol.ConnId
  , pendingXftpSharedMsg :: B.ByteString
  }

-- | Recipient FD ready after SFDONE, but slot 1 is still held by the XFTP
-- TLS session until it disconnects. Flush once the ephemeral gate is free.
data PendingXftpDescr = PendingXftpDescr
  { pendingDescrConn :: AgentProtocol.ConnId
  , pendingDescrShared :: B.ByteString
  , pendingDescrRfd :: ValidFileDescription 'FRecipient
  }

data PendingXftpRcv = PendingXftpRcv
  { pendingXftpRcvId :: B.ByteString
  , pendingXftpRcvConn :: AgentProtocol.ConnId
  , pendingXftpRcvHandle :: Word64
  , pendingXftpRcvMsgId :: Word64
  , pendingXftpRcvPart :: Word16
  , pendingXftpRcvName :: B.ByteString
  }

-- When an XFTP download finishes, re-offer the file bytes as an image payload.
-- A plain file keeps its name so the chat can show the text instead of a photo.
data XftpImageReady = XftpImageReady
  { xftpReadyHandle :: Word64
  , xftpReadyConn :: AgentProtocol.ConnId
  , xftpReadyFileId :: B.ByteString
  , xftpReadyMsgId :: Word64
  , xftpReadyPart :: Word16
  , xftpReadyBytes :: B.ByteString
  , xftpReadyName :: B.ByteString
  }

deferredXftpDescrs :: IORef [PendingXftpDescr]
deferredXftpDescrs = unsafePerformIO (newIORef [])
{-# NOINLINE deferredXftpDescrs #-}
#endif

-- The name sent in x.info and group member info. Defaults until the UI sets one.
guestProfileName :: IORef B.ByteString
guestProfileName = unsafePerformIO (newIORef "NexatrodeOS")
{-# NOINLINE guestProfileName #-}

sanitizeProfileName :: B.ByteString -> B.ByteString
sanitizeProfileName raw =
  let cleaned = B.take 48 $ B.filter (\c -> (c >= 48 && c <= 57) || (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || c == 32 || c == 45 || c == 95 || c == 46) raw
  in if B.null cleaned then "NexatrodeOS" else cleaned

loadGuestProfile :: DBStore -> IO ()
loadGuestProfile store = do
  stored <- withTransaction' store $ \db -> do
    SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_profile (id INTEGER PRIMARY KEY, display_name BLOB NOT NULL)"
    rows <- SQL.query_ db "SELECT display_name FROM guest_profile WHERE id = 1"
    pure $ case rows of
      (SQL.Only name) : _ | not (B.null (name :: B.ByteString)) -> sanitizeProfileName name
      _ -> B.empty
  when (not (B.null stored)) $ writeIORef guestProfileName stored

saveGuestProfile :: DBStore -> B.ByteString -> IO ()
saveGuestProfile store name = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_profile (id INTEGER PRIMARY KEY, display_name BLOB NOT NULL)"
  SQL.execute db "INSERT OR REPLACE INTO guest_profile (id, display_name) VALUES (1, ?)" (SQL.Only name)

-- One reusable contact address. The 1-time invitation table is separate.
loadGuestAddress :: DBStore -> IO (Maybe (AgentProtocol.ConnId, B.ByteString))
loadGuestAddress store = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_contact_address (id INTEGER PRIMARY KEY, conn_id BLOB NOT NULL, uri BLOB NOT NULL)"
  rows <- SQL.query_ db "SELECT conn_id, uri FROM guest_contact_address WHERE id = 1"
  pure $ case rows of
    (conn, uri) : _ | not (B.null (uri :: B.ByteString)) -> Just (conn, uri)
    _ -> Nothing

saveGuestAddress :: DBStore -> AgentProtocol.ConnId -> B.ByteString -> IO ()
saveGuestAddress store connectionId uri = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_contact_address (id INTEGER PRIMARY KEY, conn_id BLOB NOT NULL, uri BLOB NOT NULL)"
  SQL.execute db "INSERT OR REPLACE INTO guest_contact_address (id, conn_id, uri) VALUES (1, ?, ?)" (connectionId, uri)

ownProfileInfo :: IO AgentProtocol.ConnInfo
ownProfileInfo = do
  name <- BSC.unpack . sanitizeProfileName <$> readIORef guestProfileName
  pure $ fromString $ "{\"v\":\"1-19\",\"event\":\"x.info\",\"params\":{\"profile\":{\"displayName\":\""
    <> name <> "\",\"fullName\":\"\"}}}"

#ifdef SIMPLEX_AGENT_SERVICE_MODE
foreign import ccall unsafe "sel4_simplex_ipc_take"
  simplexIpcTake :: Ptr Word32 -> Ptr Word32 -> Ptr Word8 -> Word32 -> Ptr Word32 -> IO CInt

foreign import ccall unsafe "sel4_simplex_ipc_complete"
  simplexIpcComplete :: Word32 -> Word32 -> Word32 -> IO ()

foreign import ccall unsafe "sel4_simplex_ipc_complete_bytes"
  simplexIpcCompleteBytes :: Word32 -> Word32 -> Ptr Word8 -> Word32 -> IO ()

foreign import ccall unsafe "sel4_simplex_ipc_notify"
  simplexIpcNotify :: IO ()
#endif

main :: IO ()
main = do
  hSetBuffering stdout LineBuffering
  created <- createAgentStore
    DBOpts
      { dbFilePath = "agent.db",
        dbFunctions = [],
        dbKey = BA.convert (BSC.pack "qemu-agentstore-test-key-v1") :: ScrubbedBytes,
        keepKey = True,
        vacuum = False,
        track = TQOff
      }
    MigrationConfig {confirm = MCYesUpDown, backupPath = Nothing}
  case created of
    Left err -> print err >> putStrLn "AGENT_RUNTIME|STORE_FAIL"
    Right store -> do
      putStrLn "AGENTSTORE_MIGRATIONS|PASS"
      existing <- withTransaction store AgentStore.getUserIds
      recordId <- case existing of
        firstId : _ -> pure firstId
        [] -> withTransaction store AgentStore.createUserRecord
      -- Keep both test connections on one AgentStore user. This is the real
      -- shape of one phone account and avoids requiring concurrent TCP sessions.
      let peerId = recordId
      stored <- withTransaction store AgentStore.getUserIds
      if recordId `elem` stored
        then putStrLn ("AGENTSTORE_RECORD|PASS|" <> show recordId)
        else putStrLn "AGENTSTORE_RECORD|FAIL"
#ifdef SIMPLEX_PUBLIC_SMP
      -- One clearnet preset from the upstream SimpleX operator list. The
      -- invitation then names this public relay, so an official phone app can
      -- open it. No onion address: this guest has no Tor transport.
      let publicPin :: KeyHash
          publicPin =
            either (error . show) id $
              strDecode "u2dS9sG8nMNURyZwqASV4yROM28Er0luVTx5X1CsMrU="
          server =
            SMPServer
              (THDomainName "smp4.simplex.im" :| [])
              "5223"
              publicPin
#else
      let server = SMPServer
            (THDomainName "personal-os.test" :| [])
            "65224"
            (fixturePin caDER)
#endif
          serverCfg = presetServerCfg True allRoles Nothing (noAuthSrv server)
#ifdef SIMPLEX_PUBLIC_SMP
      putStrLn "AGENT_RUNTIME|PUBLIC_SMP|smp4.simplex.im:5223"
      -- Same public XFTP presets the official apps use. HMPublic keeps
      -- clearnet names (xftp1.simplex.im, …); this guest has no Tor.
      let xftpServers = fmap (presetServerCfg True allRoles Nothing) defaultXFTPServers
#endif
      let initialServers = InitialAgentServers
            { smp = M.singleton recordId (serverCfg :| []),
              ntf = [],
#ifdef SIMPLEX_PUBLIC_SMP
              xftp = M.singleton recordId xftpServers,
#else
              xftp = M.empty,
#endif
              -- A second relay has to be reached while the invitation queue stays
              -- subscribed. Prefer its clearnet name; this guest has no Tor.
              netCfg =
#ifdef SIMPLEX_PUBLIC_SMP
                defaultNetworkConfig {hostMode = HMPublic},
#else
                defaultNetworkConfig,
#endif
              useServices = M.empty,
              presetDomains = [],
              presetServers = []
            }
          initialServers' = initialServers
      started <- runExceptT $
        Agent.getSMPAgentClient defaultAgentConfig initialServers' store True
      case started of
        Left err -> do
          print err
          closeDBStore store
          putStrLn "AGENT_RUNTIME|START_FAIL"
        Right client -> do
#ifdef SIMPLEX_AGENT_SERVICE_MODE
          -- Keep the real upstream Agent, its SQLite store, network workers,
          -- and RTS alive as a guest service. Its first app operation reads
          -- the real encrypted AgentStore connection table.
#ifdef SIMPLEX_PUBLIC_SMP
          xftpWorkers <- runExceptT $ do
            Agent.xftpStartWorkers client (Just "xftp")
            Agent.xftpStartSndWorkers client (Just "xftp")
          case xftpWorkers of
            Left err -> putStrLn ("AGENT_RUNTIME|PUBLIC_XFTP_FAIL|" <> show err)
            Right () -> putStrLn "AGENT_RUNTIME|PUBLIC_XFTP|xftp1.simplex.im"
#endif
          putStrLn "AGENT_RUNTIME|SERVICE_READY"
          simplexAgentServiceLoop store client recordId
#else
          putStrLn "AGENT_RUNTIME|START_PASS"
          users <- withTransaction store AgentStore.getUserIds
          putStrLn ("AGENT_RUNTIME|STORE_USERS|" <> show (length users))
#ifdef SIMPLEX_AGENT_EXTERNAL_PEER
          storedConnections <- withTransaction store AgentStore.getConnIds
          roundTrip <- case storedConnections of
            receiverId : _ -> runExceptT $ do
              eventBacklog <- liftIO $ newIORef []
              liftIO $ putStrLn ("AGENT_RUNTIME|CONN_RESTORED|" <> show receiverId)
              -- Match the upstream chat client's restart path: restore all
              -- saved queue subscriptions for the persisted user before
              -- testing delivery on this connection.
              case users of
                userId : _ -> Agent.subscribeAllConnections client False (Just userId)
                [] -> throwError $ AgentProtocol.INTERNAL "restored Agent store has no user"
              -- Bulk restore reports the queue active locally but does not
              -- keep the restored server-side subscriber alive in this
              -- Microkit network path. Re-subscribe this connection before
              -- accepting inbound messages.
              Agent.subscribeConnection client receiverId
              liftIO $ putStrLn "SIMPLEX_INTEROP_RESUME_EXPLICIT_SUBSCRIBE|PASS"
              resumedStats <- Agent.getConnectionServers client receiverId
              liftIO $ putStrLn ("SIMPLEX_INTEROP_RESUME_STATS|" <> show resumedStats)
              liftIO $ putStrLn "SIMPLEX_INTEROP_RESUME_READY|PASS"
              let resumeBody = "{\"v\":\"1-19\",\"event\":\"x.msg.new\",\"params\":{\"content\":{\"type\":\"text\",\"text\":\"hello after NexatrodeOS reboot\"},\"mentions\":{}}}"
              (resumeMsgId, _) <- Agent.sendMessage client receiverId PQEncOn noMsgFlags resumeBody
              void ((awaitAgentEvent eventBacklog client receiverId (\(_, connId, event) ->
                if connId == receiverId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.SENT msgId _) | msgId == resumeMsgId -> Just ()
                  _ -> Nothing
                else Nothing)) :: ExceptT AgentProtocol.AgentErrorType IO ())
              liftIO $ putStrLn "SIMPLEX_INTEROP_RESUME_OUTBOUND_ACCEPTED|PASS"
              -- The restarted CLI peer checks whether this queued message is delivered.
              liftIO $ putStrLn ("SIMPLEX_INTEROP_RESUME_OUTBOUND_QUEUED|PASS|" <> show resumeMsgId)
              liftIO $ putStrLn "SIMPLEX_INTEROP_PERSISTENCE_PASS|same-contact-send-after-reboot"
#ifdef SIMPLEX_AGENT_EXTERNAL_BIDIRECTIONAL
              liftIO $ putStrLn "SIMPLEX_INTEROP_RESUME_READY_FOR_INBOUND|PASS"
              (incomingMsgId, incomingBody) <- (awaitAgentEvent eventBacklog client receiverId $ \(_, connId, event) ->
                if connId == receiverId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.MSG meta _ msgBody) ->
                    let (msgId, _) = AgentProtocol.recipient meta in Just (msgId, msgBody)
                  _ -> Nothing
                else Nothing) `catchError` \err -> do
                  -- Inspect the server queue only after receive delivery has
                  -- timed out; a QUE diagnostic can itself affect the live
                  -- transport session, so keep it out of the success path.
                  queueInfo <- Agent.getConnectionQueueInfo client NRMInteractive receiverId
                  liftIO $ putStrLn ("SIMPLEX_INTEROP_RESUME_QUEUE_AFTER_TIMEOUT|" <> show queueInfo)
                  throwError err
              liftIO $ putStrLn ("SIMPLEX_INTEROP_RESUME_INBOUND|" <> BSC.unpack incomingBody)
              Agent.ackMessage client receiverId incomingMsgId Nothing
              liftIO $ putStrLn "SIMPLEX_INTEROP_RESUME_INBOUND_ACK|PASS"
#endif

            [] -> runExceptT $ do
#else
          roundTrip <- runExceptT $ do
#endif
              eventBacklog <- liftIO $ newIORef []
#ifdef SIMPLEX_AGENT_EXTERNAL_PEER
              (receiverId, AgentProtocol.CCLink requestUri _) <-
                Agent.createConnection client NRMInteractive recordId True True AgentProtocol.SCMInvitation Nothing Nothing IKPQOn SMSubscribe
              liftIO $ putStrLn ("AGENT_RUNTIME|CONN_CREATED|" <> show receiverId)
              Agent.subscribeConnection client receiverId
              -- The library associates the queue with its session asynchronously;
              -- the UP event is not emitted for this already-active session.
              liftIO $ putStrLn "SIMPLEX_INTEROP_SUBSCRIBE_REQUESTED|PASS"
              liftIO $ putStrLn "SIMPLEX_INTEROP_SUBSCRIPTION_MODE|SMSubscribe"
              connectionStats <- Agent.getConnectionServers client receiverId
              liftIO $ putStrLn ("SIMPLEX_INTEROP_CONNECTION_STATS|" <> show connectionStats)
              let invitation = BSC.unpack $ strEncode requestUri
              liftIO $ putStrLn ("SIMPLEX_INTEROP_INVITE|" <> invitation)
              liftIO $ putStrLn "SIMPLEX_INTEROP_WAITING_CONF|PASS"
              (confirmationId, _) <- (awaitAgentEvent eventBacklog client receiverId (\(_, connId, event) ->
                if connId == receiverId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.CONF confirmation _ _ info) -> Just (confirmation, info)
                  _ -> Nothing
                else Nothing)) :: ExceptT AgentProtocol.AgentErrorType IO (AgentProtocol.ConfirmationId, AgentProtocol.ConnInfo)
              liftIO $ putStrLn "SIMPLEX_INTEROP_CONF_RECEIVED|PASS"
              -- SimpleX Chat expects ConnInfo to be a serialized ChatMessage.
              -- Send the public x.info profile event used by the upstream client.
              Agent.allowConnection client receiverId confirmationId
                "{\"v\":\"1-19\",\"event\":\"x.info\",\"params\":{\"profile\":{\"displayName\":\"NexatrodeOS\",\"fullName\":\"\"}}}"
              liftIO $ putStrLn "SIMPLEX_INTEROP_ALLOW_REQUESTED|PASS"
              void ((awaitAgentEvent eventBacklog client receiverId (\(_, connId, event) ->
                if connId == receiverId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.CON _) -> Just ()
                  _ -> Nothing
                else Nothing)) :: ExceptT AgentProtocol.AgentErrorType IO ())
              liftIO $ putStrLn "SIMPLEX_INTEROP_CON_RECEIVED|PASS"
              -- The invitation starts as NewConnection, where its first
              -- subscribeConnection call is a no-op. Subscribe the confirmed
              -- receive queue now that the peer accepted CONF.
              Agent.subscribeConnection client receiverId
              confirmedStats <- Agent.getConnectionServers client receiverId
              liftIO $ putStrLn ("SIMPLEX_INTEROP_CONFIRMED_STATS|" <> show confirmedStats)
              -- The CLI sends ChatMessage JSON over the Agent layer, not plain text.
              let body = "{\"v\":\"1-19\",\"event\":\"x.msg.new\",\"params\":{\"content\":{\"type\":\"text\",\"text\":\"hello from NexatrodeOS QEMU\"},\"mentions\":{}}}"
              (outgoingMsgId, _) <- Agent.sendMessage client receiverId PQEncOn noMsgFlags body
              void ((awaitAgentEvent eventBacklog client receiverId (\(_, connId, event) ->
                if connId == receiverId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.SENT msgId _) | msgId == outgoingMsgId -> Just ()
                  _ -> Nothing
                else Nothing)) :: ExceptT AgentProtocol.AgentErrorType IO ())
              liftIO $ putStrLn "SIMPLEX_INTEROP_GUEST_SEND_ACCEPTED|PASS"
              liftIO $ putStrLn ("SIMPLEX_INTEROP_GUEST_SEND_QUEUED|PASS|" <> show outgoingMsgId)
#ifdef SIMPLEX_AGENT_EXTERNAL_BIDIRECTIONAL
              liftIO $ putStrLn "SIMPLEX_INTEROP_READY_FOR_REPLY|PASS"
              incoming <- (awaitAgentEvent eventBacklog client receiverId $ \(_, connId, event) ->
                if connId == receiverId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.MSG meta _ msgBody) ->
                    let (msgId, _) = AgentProtocol.recipient meta in Just (msgId, msgBody)
                  _ -> Nothing
                else Nothing) `catchError` \err -> do
                  liftIO $ putStrLn ("SIMPLEX_INTEROP_INBOUND_PUSH_TIMEOUT|" <> show err)
                  queueInfo <- Agent.getConnectionQueueInfo client NRMInteractive receiverId
                  liftIO $ putStrLn ("SIMPLEX_INTEROP_INBOUND_TIMEOUT_QUEUE|" <> show queueInfo)
                  -- Duplicate SUB asks the broker to replay its oldest queued
                  -- message in the correlated SUB response. This distinguishes
                  -- a missing live push from a message/ratchet problem.
                  Agent.subscribeConnection client receiverId
                  liftIO $ putStrLn "SIMPLEX_INTEROP_DUPLICATE_SUBSCRIBE|PASS"
                  awaitAgentEvent eventBacklog client receiverId $ \(_, connId, event) ->
                    if connId == receiverId then case event of
                      AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.MSG meta _ msgBody) ->
                        let (msgId, _) = AgentProtocol.recipient meta in Just (msgId, msgBody)
                      _ -> Nothing
                    else Nothing
              let (incomingMsgId, incomingBody) = incoming
              liftIO $ putStrLn ("SIMPLEX_INTEROP_INBOUND|" <> BSC.unpack incomingBody)
              Agent.ackMessage client receiverId incomingMsgId Nothing
#endif
#ifdef SIMPLEX_AGENT_EXTERNAL_BIDIRECTIONAL
              pure ()
#else
              Agent.deleteConnection client NRMInteractive receiverId
#endif
#else
              (receiverId, AgentProtocol.CCLink requestUri _) <-
                Agent.createConnection client NRMInteractive recordId True True AgentProtocol.SCMInvitation Nothing Nothing IKPQOn SMSubscribe
              senderId <- Agent.prepareConnectionToJoin client peerId True requestUri PQSupportOn
              void $ Agent.joinConnection client NRMInteractive peerId senderId True requestUri "QEMU peer" PQSupportOn SMSubscribe
              (confirmationId, peerInfo) <- (awaitAgentEvent eventBacklog client receiverId (\(_, connId, event) ->
                if connId == receiverId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.CONF confirmation _ _ info) -> Just (confirmation, info)
                  _ -> Nothing
                else Nothing)) :: ExceptT AgentProtocol.AgentErrorType IO (AgentProtocol.ConfirmationId, AgentProtocol.ConnInfo)
              Agent.allowConnection client receiverId confirmationId "QEMU local user"
              void ((awaitAgentEvent eventBacklog client receiverId (\(_, connId, event) ->
                if connId == receiverId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.CON _) -> Just ()
                  _ -> Nothing
                else Nothing)) :: ExceptT AgentProtocol.AgentErrorType IO ())
              void ((awaitAgentEvent eventBacklog client senderId (\(_, connId, event) ->
                if connId == senderId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.CON _) -> Just ()
                  _ -> Nothing
                else Nothing)) :: ExceptT AgentProtocol.AgentErrorType IO ())
              let body = "hello from NexatrodeOS QEMU"
              (msgId, _) <- Agent.sendMessage client receiverId PQEncOn noMsgFlags body
              void ((awaitAgentEvent eventBacklog client receiverId (\(_, connId, event) ->
                if connId == receiverId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.SENT _ _) -> Just ()
                  _ -> Nothing
                else Nothing)) :: ExceptT AgentProtocol.AgentErrorType IO ())
              received <- (awaitAgentEvent eventBacklog client senderId $ \(_, connId, event) ->
                if connId == senderId then case event of
                  AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.MSG _ _ msgBody) -> Just msgBody
                  _ -> Nothing
                else Nothing) :: ExceptT AgentProtocol.AgentErrorType IO B.ByteString
              if received == body
                then pure ()
                else fail "upstream SimpleX message body mismatch"
              Agent.ackMessage client senderId msgId Nothing
              Agent.deleteConnection client NRMInteractive receiverId
              Agent.deleteConnection client NRMInteractive senderId
#endif
          case roundTrip of
            Right () -> do
#ifdef SIMPLEX_AGENT_EXTERNAL_PEER
              if null storedConnections
                then putStrLn "AGENT_RUNTIME|EXTERNAL_INTEROP_PASS|encrypted-cli-receive-ack"
                else putStrLn "AGENT_RUNTIME|EXTERNAL_PERSISTENCE_PASS|same-contact-send-after-reboot"
#ifdef SIMPLEX_AGENT_EXTERNAL_BIDIRECTIONAL
              if null storedConnections
                then putStrLn "AGENT_RUNTIME|EXTERNAL_BIDIRECTIONAL_PASS|encrypted-reply-received-acked"
                else putStrLn "AGENT_RUNTIME|EXTERNAL_PERSISTENCE_BIDIRECTIONAL_PASS|same-contact-inbound-after-reboot"
#endif
#else
              putStrLn "AGENT_RUNTIME|MESSAGE_TEST_PASS|encrypted-send-receive-ack"
#endif
            Left err -> putStrLn ("AGENT_RUNTIME|MESSAGE_TEST_ERROR|" <> show err) >> putStrLn "AGENT_RUNTIME|MESSAGE_TEST_FAIL"
          Agent.disposeAgentClient client
          putStrLn "AGENT_RUNTIME|DISPOSE_PASS"
#endif

#ifdef SIMPLEX_AGENT_SERVICE_MODE
-- Offset zero begins a new upload. Any invalid chunk must cause the caller to
-- discard the prior buffer rather than accidentally sending a partial file.
stageOutgoingAttachment :: B.ByteString -> B.ByteString -> Maybe B.ByteString
stageOutgoingAttachment old payload
  | B.length payload <= 4 = Nothing
  | B.length chunk > 3072 = Nothing
  | offset /= B.length base = Nothing
  | B.length base + B.length chunk > 1048576 = Nothing
  | otherwise = Just (base <> chunk)
  where
    offset = sum [fromIntegral (B.index payload i) `shiftL` (8 * i) | i <- [0..3]]
    chunk = B.drop 4 payload
    base = if offset == 0 then B.empty else old

simplexAgentServiceLoop :: DBStore -> Agent.AgentClient -> AgentProtocol.UserId -> IO ()
simplexAgentServiceLoop store client userId = do
  eventBacklog <- newIORef []
  deliveryReceipts <- newIORef []
  imageBytes <- newIORef B.empty
  outgoingAttachment <- newIORef B.empty
#ifdef SIMPLEX_ATTACHMENT_SELF_TEST
  let sample = B.replicate 3072 165
      add prior n = prior >>= \old -> stageOutgoingAttachment old (B.pack (word32LE (fromIntegral (n * 3072))) <> sample)
      almost = foldl' add (Just B.empty) [0..340 :: Int]
      complete = almost >>= \old -> stageOutgoingAttachment old (B.pack (word32LE (341 * 3072)) <> B.replicate 1024 165)
      overflow = complete >>= \old -> stageOutgoingAttachment old (B.pack (word32LE 1048576) <> B.singleton 1)
      skipped = stageOutgoingAttachment B.empty (B.pack (word32LE 3072) <> B.singleton 1)
      oversized = stageOutgoingAttachment B.empty (B.pack (word32LE 0) <> B.replicate 3073 1)
      reset = stageOutgoingAttachment (B.replicate 32 7) (B.pack (word32LE 0) <> B.singleton 9)
  if maybe False ((== 1048576) . B.length) complete && overflow == Nothing && skipped == Nothing && oversized == Nothing && reset == Just (B.singleton 9)
    then putStrLn "SIMPLEXIPC|UPLOAD_STAGE_TEST|PASS"
    else putStrLn "SIMPLEXIPC|UPLOAD_STAGE_TEST|FAIL"
  workFile <- Exception.try (do
    createDirectoryIfMissing True "xftp"
    let path = "xftp/one-mebibyte-selftest"
        bytes = B.replicate 1048576 0xa5
    B.writeFile path bytes
    actual <- B.readFile path
    removeFile path
    pure (actual == bytes)) :: IO (Either Exception.SomeException Bool)
  putStrLn $ case workFile of
    Right True -> "SIMPLEXIPC|XFTP_WORKFILE_1M|PASS"
    _ -> "SIMPLEXIPC|XFTP_WORKFILE_1M|FAIL"
#endif
  groupInvites <- newIORef []
  loadGroupInvites store >>= writeIORef groupInvites
  groupSelves <- newIORef []
  loadGroupSelves store >>= writeIORef groupSelves
  groupLinks <- newIORef []
  loadGroupLinks store >>= writeIORef groupLinks
  loadGuestProfile store
  pendingPieces <- newIORef []
#ifdef SIMPLEX_PUBLIC_SMP
  pendingXftp <- newIORef ([] :: [PendingXftpSend])
  pendingXftpRcv <- newIORef ([] :: [PendingXftpRcv])
  xftpImageReady <- newIORef (Seq.empty :: Seq.Seq XftpImageReady)
  xftpInFlight <- newIORef Nothing
  restoreXftpReceives store client userId pendingXftpRcv
#endif
  -- A joined contact lives in the encrypted store. Resume its queues, including
  -- a peer relay that is not the invitation server, before the UI asks.
  restored <- runExceptT $ Agent.subscribeAllConnections client False (Just userId)
  case restored of
    Left err -> putStrLn ("SIMPLEXIPC|GUEST_AGENT_RESTORE_FAIL|" <> show err)
    Right () -> putStrLn "SIMPLEXIPC|GUEST_AGENT_RESTORE_PASS"
  allocaBytes 4076 $ \payloadPtr -> forever $ do
#ifdef SIMPLEX_PUBLIC_SMP
    processAvailableAgentEvents store client eventBacklog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady
    flushDeferredXftpDescrs client eventBacklog
#else
    processAvailableAgentEvents store client eventBacklog deliveryReceipts
#endif
    pokeInbox eventBacklog
    next <- alloca $ \operationPtr -> alloca $ \requestIdPtr -> alloca $ \payloadLengthPtr -> do
      result <- simplexIpcTake operationPtr requestIdPtr payloadPtr 4076 payloadLengthPtr
      if result <= 0
        then pure Nothing
        else do
          operation <- peek operationPtr
          requestId <- peek requestIdPtr
          payloadLength <- peek payloadLengthPtr
          payload <- B.packCStringLen (castPtr payloadPtr, fromIntegral payloadLength)
          pure $ Just (operation, requestId, payload)
    case next of
#ifdef SIMPLEX_PUBLIC_SMP
      Nothing -> waitForRelay store client eventBacklog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady
#else
      Nothing -> waitForRelay store client eventBacklog deliveryReceipts
#endif
      Just (operation, requestId, requestPayload)
        | operation == 1 -> do
            connectionIds <- withTransaction store AgentStore.getConnIds
            -- Newest saved profile first, so the Mac app is its own chat
            -- ahead of an older contact such as Phone. Unnamed connections
            -- stay in the handle list after the named chats.
            namedChats <- rememberedChats store connectionIds
            selves <- readIORef groupSelves
            -- Direct contacts keep a row even when older test groups refill
            -- the newest-name window. Groups follow, newest first, and the
            -- snapshot still stops at 16 handles.
            let isGroup connectionId = any ((== connectionId) . groupSelfHost) selves
                directChats = filter (not . isGroup . fst) namedChats
                groupChats = filter (isGroup . fst) namedChats
            chats <- if null namedChats
              then fmap (maybe [] (: [])) (firstPeerName store connectionIds)
              else pure $ take 16 (directChats ++ groupChats)
            seedPeerImages store connectionIds
            let chatIds = map fst chats
                ordered = take 16 (chatIds ++ filter (`notElem` chatIds) connectionIds)
                -- The row order from before direct contacts were pinned first.
                -- The messages archive uses it once to put old lines back.
                legacyIds = take 16 (map fst namedChats ++ filter (`notElem` map fst namedChats) connectionIds)
                legacyHandles = map stableConnectionHandle legacyIds
                    ++ replicate (16 - length legacyIds) 0
                handles = map stableConnectionHandle ordered
                count = fromIntegral (length handles) :: Word16
                namePayload = B.singleton (fromIntegral (length chats) :: Word8) <> B.concat
                  [ let name' = B.take 32 name
                    in B.singleton (fromIntegral (B.length name') :: Word8) <> name'
                  | (_, name) <- chats
                  ]
            inviteSuffix <- groupInviteSuffix chats groupInvites
            photoMask <- peerPhotoMask store chats
            let groupMask :: Word16
                groupMask = foldl'
                  (\bits (index, (connectionId, _)) ->
                      if any ((== connectionId) . groupSelfHost) selves
                        then bits .|. (1 `shiftL` index)
                        else bits)
                  0
                  (zip [0 :: Int ..] chats)
                payload = B.pack (word16LE count <> concatMap word64LE handles)
                  <> namePayload
                  <> inviteSuffix
                  <> B.pack (word16LE groupMask <> word16LE photoMask <> concatMap word64LE legacyHandles)
            B.useAsCStringLen payload $ \(bytes, length_) ->
              simplexIpcCompleteBytes requestId 1 (castPtr bytes) (fromIntegral length_)
            putStrLn ("SIMPLEXIPC|GUEST_AGENT_CHAT_COUNT|" <> show (length chats))
            forM_ (zip [0 :: Int ..] chats) $ \(index, (_, name)) ->
              putStrLn ("SIMPLEXIPC|GUEST_AGENT_CHAT|" <> show index <> "|" <> BSC.unpack name)
            putStrLn ("SIMPLEXIPC|GUEST_AGENT_GROUP_MASK|" <> show groupMask)
            putStrLn ("SIMPLEXIPC|GUEST_AGENT_PHOTO_MASK|" <> show photoMask)
            putStrLn ("SIMPLEXIPC|GUEST_AGENT_LIST_CONNECTIONS_PASS|" <> show count)
        | operation == 2 -> do
            invitation <- runExceptT $
              Agent.createConnection client NRMInteractive userId True True
                AgentProtocol.SCMInvitation Nothing Nothing IKPQOn SMSubscribe
            case invitation of
              Left _ -> do
                simplexIpcCompleteBytes requestId 5 nullPtr 0
                putStrLn "SIMPLEXIPC|GUEST_AGENT_CREATE_INVITATION_FAIL"
              Right (connectionId, AgentProtocol.CCLink requestUri _) -> do
                let uri = strEncode requestUri
                    handle = stableConnectionHandle connectionId
                    payload = B.pack (word64LE handle <> word16LE (fromIntegral (B.length uri))) <> uri
                B.useAsCStringLen payload $ \(bytes, length_) ->
                  simplexIpcCompleteBytes requestId 1 (castPtr bytes) (fromIntegral length_)
                putStrLn ("SIMPLEXIPC|GUEST_AGENT_CREATE_INVITATION_PASS|" <> show (B.length uri))
#ifdef SIMPLEX_PUBLIC_SMP
                -- Bearer link for the official phone app. This QEMU image is
                -- the bring-up session; do not ship this print on a device.
                putStrLn ("SIMPLEX_PUBLIC_INVITE|" <> BSC.unpack uri)
#endif
        | operation == 3 -> case decodeInvitationPayload requestPayload of
            Nothing -> do
              simplexIpcComplete requestId 3 0
              putStrLn "SIMPLEXIPC|GUEST_AGENT_JOIN_INVITATION_INVALID"
            Just uriBytes -> case strDecode uriBytes of
              Left _ -> do
                simplexIpcComplete requestId 3 0
                putStrLn "SIMPLEXIPC|GUEST_AGENT_JOIN_INVITATION_INVALID_URI"
              Right (AgentProtocol.ACR _ requestUri) -> do
                prepared <- runExceptT $
                  Agent.prepareConnectionToJoin client userId True requestUri PQSupportOn
                case prepared of
                  Left prepareErr -> do
                    simplexIpcComplete requestId 5 0
                    putStrLn ("SIMPLEXIPC|GUEST_AGENT_JOIN_INVITATION_FAIL|prepare|" <> show prepareErr)
                  Right connectionId -> do
                    (hello, groupJoin) <- groupJoinHello store groupInvites uriBytes
                    joined <- runExceptT $ Agent.joinConnection client NRMInteractive userId
                      connectionId True requestUri hello PQSupportOn SMSubscribe
                    case joined of
                      Left joinErr -> do
                        void $ runExceptT $ Agent.deleteConnection client NRMInteractive connectionId
                        simplexIpcComplete requestId 5 0
                        putStrLn ("SIMPLEXIPC|GUEST_AGENT_JOIN_INVITATION_FAIL|join|" <> show joinErr)
                      Right _ -> do
                        case groupJoin of
                          Just (member, name) -> do
                            savePeerName store connectionId name
                            saveGroupSelf store groupSelves connectionId member name
                            forgetGroupInvite store groupInvites uriBytes
                            putStrLn "SIMPLEXIPC|GUEST_AGENT_GROUP_JOIN_PASS"
                          Nothing -> pure ()
                        let handle = B.pack (word64LE (stableConnectionHandle connectionId))
                        B.useAsCStringLen handle $ \(bytes, length_) ->
                          simplexIpcCompleteBytes requestId 1 (castPtr bytes) (fromIntegral length_)
                        putStrLn "SIMPLEXIPC|GUEST_AGENT_JOIN_INVITATION_PASS"
        | operation == 13 -> do
            -- Stage ordered chunks in a separate buffer from received images.
            -- An invalid chunk invalidates the entire upload rather than
            -- permitting a later request to send stale or partial data.
            old <- readIORef outgoingAttachment
            case stageOutgoingAttachment old requestPayload of
              Nothing -> writeIORef outgoingAttachment B.empty >> simplexIpcComplete requestId 3 0
              Just bytes -> writeIORef outgoingAttachment bytes >> simplexIpcComplete requestId 1 0
        | operation == 14 -> case decodeSendMessagePayload requestPayload of
            Nothing -> writeIORef outgoingAttachment B.empty >> simplexIpcComplete requestId 3 0
            Just (handle, nameBytes) -> do
#ifdef SIMPLEX_PUBLIC_SMP
              bytes <- readIORef outgoingAttachment
              busy <- readIORef pendingXftp
              connections <- withTransaction store AgentStore.getConnIds
              case (find ((== handle) . stableConnectionHandle) connections, TE.decodeUtf8' nameBytes) of
                (Just connectionId, Right name)
                  | not (B.null bytes) && B.length bytes <= 1048576
                  , not (Text.null name) && B.length nameBytes <= 120
                  , not (Text.any (\c -> c == '/' || c == '\\' || c < ' ') name)
                  , null busy -> do
                      createDirectoryIfMissing True "xftp"
                      let path = "xftp/outgoing-attachment" :: String
                      B.writeFile path bytes
                      writeIORef outgoingAttachment B.empty
                      sharedRaw <- getEntropy 12 :: IO ScrubbedBytes
                      let sharedId = TE.decodeUtf8 (strEncode (BA.convert sharedRaw :: B.ByteString))
                          fileSize = fromIntegral (B.length bytes) :: Integer
                          fileField = Aeson.object
                            [ "fileName" .= name, "fileSize" .= fileSize
                            , "fileDescr" .= Aeson.object
                                [ "fileDescrText" .= ("" :: Text.Text)
                                , "fileDescrPartNo" .= (0 :: Int)
                                , "fileDescrComplete" .= False ] ]
                          content = Aeson.object ["type" .= ("file" :: String), "text" .= ("" :: Text.Text)]
                          params = Aeson.object ["content" .= content, "mentions" .= Aeson.object [], "file" .= fileField]
                          message = BL.toStrict . Aeson.encode $ Aeson.object
                            [ "v" .= ("1-19" :: String), "event" .= ("x.msg.new" :: String)
                            , "params" .= params, "msgId" .= sharedId ]
                      sentId <- agentSendToTargets requestId client connectionId groupSelves groupLinks message sharedId
                      forM_ sentId $ \messageId -> do
                        waited <- runExceptT $
                          awaitAgentEvent eventBacklog client connectionId $ \(_, connId, evt) ->
                            if connId == connectionId then case evt of
                              AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.SENT mid _)
                                | mid == messageId -> Just ()
                              _ -> Nothing
                            else Nothing
                        case waited of
                          Left err -> putStrLn ("SIMPLEXIPC|GUEST_AGENT_ATTACHMENT_SENT_FAIL|" <> show err)
                          Right () -> do
                            flushBacklogSideEffects store client eventBacklog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady
                            threadDelay 500000
                            startXftpFile client userId pendingXftp connectionId (BA.convert sharedRaw) fileSize path
                _ -> writeIORef outgoingAttachment B.empty >> simplexIpcComplete requestId 3 0
#else
              let _ = (handle, nameBytes)
              writeIORef outgoingAttachment B.empty
              simplexIpcComplete requestId 5 0
#endif
        | operation == 4 -> case decodeSendMessagePayload requestPayload of
            Nothing -> simplexIpcComplete requestId 3 0
            Just (handle, textBytes) -> case TE.decodeUtf8' textBytes of
              Left _ -> simplexIpcComplete requestId 3 0
              Right rawText -> do
                connectionIds <- withTransaction store AgentStore.getConnIds
                case find ((== handle) . stableConnectionHandle) connectionIds of
                  Nothing -> simplexIpcComplete requestId 3 0
                  Just connectionId -> do
                    let wire = parseSendWire rawText
                    case wire of
                      SendEdit targetMsgId newText -> do
                        let content = Aeson.object
                              [ "type" .= ("text" :: String)
                              , "text" .= newText
                              ]
                            params = Aeson.object
                              [ "msgId" .= targetMsgId
                              , "content" .= content
                              ]
                            body = BL.toStrict . Aeson.encode $ Aeson.object
                              [ "v" .= ("1-19" :: String)
                              , "event" .= ("x.msg.update" :: String)
                              , "params" .= params
                              ]
                        putStrLn "SIMPLEXIPC|GUEST_AGENT_SEND_EDIT"
                        void $ agentSendToTargets
                          requestId client connectionId groupSelves groupLinks body Text.empty
                      SendDelete targetMsgId -> do
                        let params = Aeson.object [ "msgId" .= targetMsgId ]
                            body = BL.toStrict . Aeson.encode $ Aeson.object
                              [ "v" .= ("1-19" :: String)
                              , "event" .= ("x.msg.del" :: String)
                              , "params" .= params
                              ]
                        putStrLn "SIMPLEXIPC|GUEST_AGENT_SEND_DELETE"
                        void $ agentSendToTargets
                          requestId client connectionId groupSelves groupLinks body Text.empty
                      SendReact targetMsgId emoji add -> do
#ifdef SIMPLEX_PUBLIC_SMP
                        sharedRaw <- getEntropy 12 :: IO ScrubbedBytes
                        let sharedMsgId = TE.decodeUtf8 (strEncode (BA.convert sharedRaw :: B.ByteString))
#else
                        let sharedMsgId = Text.empty :: Text.Text
#endif
                        let reaction = Aeson.object
                              [ "type" .= ("emoji" :: String)
                              , "emoji" .= emoji
                              ]
                            params = Aeson.object
                              [ "msgId" .= targetMsgId
                              , "reaction" .= reaction
                              , "add" .= add
                              ]
                            body = BL.toStrict . Aeson.encode $ Aeson.object $
                              [ "v" .= ("1-19" :: String)
                              , "event" .= ("x.msg.react" :: String)
                              , "params" .= params
                              ]
                              ++ [ "msgId" .= sharedMsgId | not (Text.null sharedMsgId) ]
                        putStrLn
                          ( if add
                              then "SIMPLEXIPC|GUEST_AGENT_SEND_REACT"
                              else "SIMPLEXIPC|GUEST_AGENT_SEND_REACT_REMOVE"
                          )
                        void $ agentSendToTargets
                          requestId client connectionId groupSelves groupLinks body Text.empty
                      SendFile fileName fileBody -> do
#ifdef SIMPLEX_PUBLIC_SMP
                        createDirectoryIfMissing True "xftp"
                        let bytes = TE.encodeUtf8 fileBody
                            path = "xftp/note.txt" :: String
                        B.writeFile path bytes
                        sharedRaw <- getEntropy 12 :: IO ScrubbedBytes
                        let sharedMsgId = TE.decodeUtf8 (strEncode (BA.convert sharedRaw :: B.ByteString))
                            fileSize = fromIntegral (B.length bytes) :: Integer
                            fileField = Aeson.object
                              [ "fileName" .= fileName
                              , "fileSize" .= fileSize
                              , "fileDescr" .= Aeson.object
                                  [ "fileDescrText" .= ("" :: Text.Text)
                                  , "fileDescrPartNo" .= (0 :: Int)
                                  , "fileDescrComplete" .= False
                                  ]
                              ]
                            content = Aeson.object
                              [ "type" .= ("file" :: String)
                              , "text" .= fileBody
                              ]
                            params = Aeson.object
                              [ "content" .= content
                              , "mentions" .= Aeson.object []
                              , "file" .= fileField
                              ]
                            fileMsg = BL.toStrict . Aeson.encode $ Aeson.object
                              [ "v" .= ("1-19" :: String)
                              , "event" .= ("x.msg.new" :: String)
                              , "params" .= params
                              , "msgId" .= sharedMsgId
                              ]
                        putStrLn ("SIMPLEXIPC|GUEST_AGENT_SEND_FILE|" <> Text.unpack fileName)
                        sentId <- agentSendToTargets
                          requestId client connectionId groupSelves groupLinks fileMsg sharedMsgId
                        forM_ sentId $ \messageId -> do
                          putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_WAIT_SENT|" <> show messageId)
                          waited <- runExceptT $
                            awaitAgentEvent eventBacklog client connectionId $ \(_, connId, evt) ->
                              if connId == connectionId
                                then case evt of
                                  AgentProtocol.AEvt AgentProtocol.SAEConn
                                    (AgentProtocol.SENT mid _)
                                      | mid == messageId -> Just ()
                                  _ -> Nothing
                                else Nothing
                          case waited of
                            Left err ->
                              putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_WAIT_SENT_FAIL|" <> show err)
                            Right () -> do
                              putStrLn "SIMPLEXIPC|GUEST_AGENT_XFTP_WAIT_SENT_PASS"
                              flushBacklogSideEffects store client eventBacklog deliveryReceipts
                                pendingXftp pendingXftpRcv xftpImageReady
                              threadDelay 500000
                              startXftpFile client userId pendingXftp connectionId (BA.convert sharedRaw) fileSize path
#else
                        -- XFTP is only initialized by the public-relay build.
                        -- Fail the request rather than reporting an unsent file.
                        let _ = (fileName, fileBody)
                        simplexIpcComplete requestId 5 0
                        putStrLn "SIMPLEXIPC|GUEST_AGENT_SEND_FILE_UNAVAILABLE"
#endif
                      SendNew text quoteField forwarded -> do
                        let imageMessage = "data:image/" `Text.isPrefixOf` text
                            content
                              | imageMessage = Aeson.object
                                  [ "type" .= ("image" :: String)
                                  , "text" .= ("" :: Text.Text)
                                  , "image" .= text
                                  ]
                              | otherwise = Aeson.object
                                  [ "type" .= ("text" :: String)
                                  , "text" .= text
                                  ]
#ifdef SIMPLEX_PUBLIC_SMP
                        offer <-
                          if imageMessage
                            then prepareImageXftpOffer text
                            else pure Nothing
                        sharedMsgId <- case offer of
                          Just (mid, _, _, _) -> pure mid
                          Nothing -> do
                            sharedRaw <- getEntropy 12 :: IO ScrubbedBytes
                            pure (TE.decodeUtf8 (strEncode (BA.convert sharedRaw :: B.ByteString)))
                        let fileField = fmap (\(_, field, _, _) -> field) offer
#else
                        let sharedMsgId = Text.empty :: Text.Text
                            fileField = Nothing :: Maybe Aeson.Value
                            offer = Nothing :: Maybe (Text.Text, Aeson.Value, B.ByteString, Integer)
#endif
                        let params = Aeson.object $
                              [ "content" .= content
                              , "mentions" .= Aeson.object []
                              ]
                              ++ [ "forward" .= True | forwarded ]
                              ++ maybe [] (\file -> ["file" .= file]) fileField
                              ++ maybe [] (\quote -> ["quote" .= quote]) quoteField
                            body = BL.toStrict . Aeson.encode $ Aeson.object $
                              [ "v" .= ("1-19" :: String)
                              , "event" .= ("x.msg.new" :: String)
                              , "params" .= params
                              ]
                              ++ [ "msgId" .= sharedMsgId | not (Text.null sharedMsgId) ]
                        when imageMessage $ putStrLn "SIMPLEXIPC|GUEST_AGENT_SEND_IMAGE"
                        when forwarded $ putStrLn "SIMPLEXIPC|GUEST_AGENT_SEND_FORWARD"
                        sentId <- agentSendToTargets
                          requestId client connectionId groupSelves groupLinks body sharedMsgId
#ifdef SIMPLEX_PUBLIC_SMP
                        forM_ ((,) <$> sentId <*> offer) $ \(messageId, (_, _, sharedRaw, fileSize)) -> do
                          putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_WAIT_SENT|" <> show messageId)
                          waited <- runExceptT $
                            awaitAgentEvent eventBacklog client connectionId $ \(_, connId, evt) ->
                              if connId == connectionId
                                then case evt of
                                  AgentProtocol.AEvt AgentProtocol.SAEConn
                                    (AgentProtocol.SENT mid _)
                                      | mid == messageId -> Just ()
                                  _ -> Nothing
                                else Nothing
                          case waited of
                            Left err ->
                              putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_WAIT_SENT_FAIL|" <> show err)
                            Right () -> do
                              putStrLn "SIMPLEXIPC|GUEST_AGENT_XFTP_WAIT_SENT_PASS"
                              flushBacklogSideEffects store client eventBacklog deliveryReceipts
                                pendingXftp pendingXftpRcv xftpImageReady
                              threadDelay 500000
                              startImageXftp client userId pendingXftp connectionId sharedRaw fileSize
#else
                        void (pure (sentId, offer))
#endif
#ifdef SIMPLEX_PUBLIC_SMP
        | operation == 5 -> receiveNextMessage requestId eventBacklog deliveryReceipts imageBytes groupInvites groupSelves groupLinks pendingPieces pendingXftp pendingXftpRcv xftpImageReady xftpInFlight
#else
        | operation == 5 -> receiveNextMessage requestId eventBacklog deliveryReceipts imageBytes groupInvites groupSelves groupLinks pendingPieces
#endif
        | operation == 8 -> sendImageChunk requestId requestPayload imageBytes
        | operation == 9 -> case decodePeerImagePayload requestPayload of
            Nothing -> simplexIpcComplete requestId 3 0
            Just (handle, offset) -> do
              connectionIds <- withTransaction store AgentStore.getConnIds
              case find ((== handle) . stableConnectionHandle) connectionIds of
                Nothing -> simplexIpcComplete requestId 5 0
                Just connectionId -> do
                  bytes <- loadPeerImage store connectionId
                  if B.null bytes || B.length bytes > 12288 || offset > B.length bytes
                    then do
                      simplexIpcComplete requestId 2 0
                      putStrLn "SIMPLEXIPC|GUEST_AGENT_PEER_IMAGE_EMPTY"
                    else do
                      let chunk = B.take 3000 (B.drop offset bytes)
                      B.useAsCStringLen chunk $ \(ptr, length_) ->
                        simplexIpcCompleteBytes requestId 1 (castPtr ptr) (fromIntegral length_)
                      putStrLn ("SIMPLEXIPC|GUEST_AGENT_PEER_IMAGE_PASS|" <> show offset <> "|" <> show (B.length chunk) <> "|" <> show (B.length bytes))
        | operation == 6 -> case decodeAckPayload requestPayload of
            Nothing -> simplexIpcComplete requestId 3 0
            Just (handle, messageId) -> do
              connectionIds <- withTransaction store AgentStore.getConnIds
              case find ((== handle) . stableConnectionHandle) connectionIds of
                Nothing -> simplexIpcComplete requestId 3 0
                Just connectionId -> do
                  more <- batchHasMore pendingPieces connectionId messageId
                  -- Each part is committed by Messages before it advances
                  -- to the next one. Only the final part removes the relay item.
                  when (not more) $ void $ runExceptT $
                    Agent.ackMessage client connectionId (fromIntegral messageId) Nothing
#ifdef SIMPLEX_PUBLIC_SMP
                  ready <- readIORef xftpInFlight
                  case ready of
                    Just XftpImageReady {xftpReadyConn, xftpReadyFileId, xftpReadyMsgId, xftpReadyPart}
                      | xftpReadyConn == connectionId && xftpReadyMsgId == messageId -> do
                          withTransaction' store $ \db ->
                            SQL.execute db "DELETE FROM guest_xftp_inbox WHERE conn_id = ? AND msg_id = ? AND part = ?"
                              (connectionId, fromIntegral messageId :: Int64, fromIntegral xftpReadyPart :: Int64)
                          writeIORef xftpInFlight Nothing
                          Agent.xftpDeleteRcvFile client xftpReadyFileId
                    _ -> pure ()
#endif
                  simplexIpcCompleteBytes requestId 1 nullPtr 0
                  putStrLn "SIMPLEXIPC|GUEST_AGENT_ACKNOWLEDGE_PASS"
        | operation == 10 -> do
            let name = sanitizeProfileName requestPayload
            writeIORef guestProfileName name
            saveGuestProfile store name
            B.useAsCStringLen name $ \(ptr, length_) ->
              simplexIpcCompleteBytes requestId 1 (castPtr ptr) (fromIntegral length_)
            putStrLn ("SIMPLEXIPC|GUEST_AGENT_PROFILE|" <> BSC.unpack name)
#ifdef SIMPLEX_PUBLIC_SMP
        | operation == 11 -> case decodeCreateGroupPayload requestPayload of
            Nothing -> do
              simplexIpcComplete requestId 3 0
              putStrLn "SIMPLEXIPC|GUEST_AGENT_CREATE_GROUP_INVALID"
            Just (handle, nameBytes) -> do
              connectionIds <- withTransaction store AgentStore.getConnIds
              case find ((== handle) . stableConnectionHandle) connectionIds of
                Nothing -> do
                  simplexIpcComplete requestId 3 0
                  putStrLn "SIMPLEXIPC|GUEST_AGENT_CREATE_GROUP_NO_CONTACT"
                Just contactId -> do
                  ownerRaw <- getEntropy 12 :: IO ScrubbedBytes
                  memberRaw <- getEntropy 12 :: IO ScrubbedBytes
                  let ownerId = TE.decodeUtf8 (strEncode (BA.convert ownerRaw :: B.ByteString))
                      memberId = TE.decodeUtf8 (strEncode (BA.convert memberRaw :: B.ByteString))
                      groupName = case TE.decodeUtf8' (sanitizeGroupName nameBytes) of
                        Right text | not (Text.null text) -> text
                        _ -> "Lab"
                  created <- runExceptT $
                    Agent.createConnection client NRMInteractive userId True True
                      AgentProtocol.SCMInvitation Nothing Nothing IKPQOn SMSubscribe
                  case created of
                    Left err -> do
                      simplexIpcComplete requestId 5 0
                      putStrLn ("SIMPLEXIPC|GUEST_AGENT_CREATE_GROUP_FAIL|conn|" <> show err)
                    Right (groupConn, AgentProtocol.CCLink requestUri _) -> do
                      let uri = strEncode requestUri
                          body = BL.toStrict . Aeson.encode $ Aeson.object
                            [ "v" .= ("1-19" :: String)
                            , "event" .= ("x.grp.inv" :: String)
                            , "params" .= Aeson.object
                                [ "groupInvitation" .= Aeson.object
                                    [ "fromMember" .= Aeson.object
                                        [ "memberId" .= ownerId
                                        , "memberRole" .= ("owner" :: String)
                                        ]
                                    , "invitedMember" .= Aeson.object
                                        [ "memberId" .= memberId
                                        , "memberRole" .= ("member" :: String)
                                        ]
                                    , "connRequest" .= TE.decodeUtf8 uri
                                    , "groupProfile" .= Aeson.object
                                        [ "displayName" .= groupName
                                        , "fullName" .= ("" :: String)
                                        ]
                                    , "groupSize" .= (1 :: Int)
                                    ]
                                ]
                            ]
                      sent <- runExceptT $
                        Agent.sendMessage client contactId PQEncOn noMsgFlags body
                      case sent of
                        Left sendErr -> do
                          void $ runExceptT $ Agent.deleteConnection client NRMInteractive groupConn
                          simplexIpcComplete requestId 5 0
                          putStrLn ("SIMPLEXIPC|GUEST_AGENT_CREATE_GROUP_FAIL|send|" <> show sendErr)
                        Right _ -> do
                          let nameStored = TE.encodeUtf8 groupName
                          savePeerName store groupConn nameStored
                          saveGroupSelf store groupSelves groupConn (TE.encodeUtf8 ownerId) nameStored
                          saveHostedGroup store groupConn
                          let replyHandle = B.pack (word64LE (stableConnectionHandle groupConn))
                          B.useAsCStringLen replyHandle $ \(bytes, length_) ->
                            simplexIpcCompleteBytes requestId 1 (castPtr bytes) (fromIntegral length_)
                          putStrLn ("SIMPLEXIPC|GUEST_AGENT_CREATE_GROUP_PASS|" <> Text.unpack groupName)
#endif
        | operation == 12 -> do
            let replyAddress connectionId uri = do
                  let handle = stableConnectionHandle connectionId
                      payload = B.pack (word64LE handle <> word16LE (fromIntegral (B.length uri))) <> uri
                  B.useAsCStringLen payload $ \(bytes, length_) ->
                    simplexIpcCompleteBytes requestId 1 (castPtr bytes) (fromIntegral length_)
            stored <- loadGuestAddress store
            case stored of
              Just (connectionId, uri) -> do
                replyAddress connectionId uri
                putStrLn ("SIMPLEXIPC|GUEST_AGENT_ADDRESS_READY|" <> show (B.length uri))
#ifdef SIMPLEX_PUBLIC_SMP
                putStrLn ("SIMPLEX_PUBLIC_ADDRESS|" <> BSC.unpack uri)
#endif
              Nothing -> do
                created <- runExceptT $
                  Agent.createConnection client NRMInteractive userId True True
                    AgentProtocol.SCMContact Nothing Nothing IKPQOff SMSubscribe
                case created of
                  Left err -> do
                    simplexIpcCompleteBytes requestId 5 nullPtr 0
                    putStrLn ("SIMPLEXIPC|GUEST_AGENT_ADDRESS_FAIL|" <> show err)
                  Right (connectionId, AgentProtocol.CCLink requestUri _) -> do
                    let uri = strEncode requestUri
                    saveGuestAddress store connectionId uri
                    replyAddress connectionId uri
                    putStrLn ("SIMPLEXIPC|GUEST_AGENT_ADDRESS_PASS|" <> show (B.length uri))
#ifdef SIMPLEX_PUBLIC_SMP
                    putStrLn ("SIMPLEX_PUBLIC_ADDRESS|" <> BSC.unpack uri)
#endif
        | operation == 7 -> case decodeDeletePayload requestPayload of
            Nothing -> simplexIpcComplete requestId 3 0
            Just handle -> do
              connectionIds <- withTransaction store AgentStore.getConnIds
              case find ((== handle) . stableConnectionHandle) connectionIds of
                Nothing -> simplexIpcComplete requestId 3 0
                Just connectionId -> do
                  deleted <- runExceptT $ Agent.deleteConnection client NRMInteractive connectionId
                  case deleted of
                    Left _ -> do
                      simplexIpcComplete requestId 5 0
                      putStrLn "SIMPLEXIPC|GUEST_AGENT_DELETE_CONNECTION_FAIL"
                    Right () -> do
                      simplexIpcCompleteBytes requestId 1 nullPtr 0
                      putStrLn "SIMPLEXIPC|GUEST_AGENT_DELETE_CONNECTION_PASS"
        | otherwise -> simplexIpcComplete requestId 3 0
  where
#ifdef SIMPLEX_PUBLIC_SMP
    receiveNextMessage requestId eventBacklog deliveryReceipts imageBytes groupInvites groupSelves groupLinks pendingPieces pendingXftp pendingXftpRcv xftpImageReady xftpInFlight = do
#else
    receiveNextMessage requestId eventBacklog deliveryReceipts imageBytes groupInvites groupSelves groupLinks pendingPieces = do
#endif
      -- The SMP subscription already pushes messages into the agent queue.
      -- This read only takes what is waiting. It must return immediately so a
      -- send is not stuck behind a 30 second poll.
#ifdef SIMPLEX_PUBLIC_SMP
      processAvailableAgentEvents store client eventBacklog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady
#else
      processAvailableAgentEvents store client eventBacklog deliveryReceipts
#endif
      connectionIds <- withTransaction store AgentStore.getConnIds
      deliver connectionIds 8
      where
        deliver _ 0 = simplexIpcComplete requestId 2 0
        deliver connectionIds remaining = do
#ifdef SIMPLEX_PUBLIC_SMP
          current <- readIORef xftpInFlight
          ready <- case current of
            Just _ -> pure current
            Nothing -> atomicModifyIORef' xftpImageReady $ \pending ->
              case Seq.viewl pending of
                Seq.EmptyL -> (pending, Nothing)
                next Seq.:< rest -> (rest, Just next)
          case ready of
            Just item@XftpImageReady {xftpReadyHandle, xftpReadyMsgId, xftpReadyPart, xftpReadyBytes, xftpReadyName} -> do
              writeIORef xftpInFlight (Just item)
              let photo = B.isPrefixOf "\xff\xd8" xftpReadyBytes
                  size = B.length xftpReadyBytes
              if size <= 1048576
                then do
                  writeIORef imageBytes xftpReadyBytes
                  let first = B.take 3000 xftpReadyBytes
                      name = if photo then B.empty else B.take 240 xftpReadyName
                      kind = if photo then 3 else 6
                      payload = B.pack
                        (kind : word64LE xftpReadyHandle <> word64LE xftpReadyMsgId
                          <> word16LE (fromIntegral (B.length name)))
                        <> name
                        <> B.pack (word16LE 0 <> word32LE (fromIntegral size)
                          <> word16LE (fromIntegral (B.length first)))
                        <> first <> B.pack (word16LE xftpReadyPart)
                  B.useAsCStringLen payload $ \(ptr, length_) ->
                    simplexIpcCompleteBytes requestId 1 (castPtr ptr) (fromIntegral length_)
                  putStrLn ((if photo then "SIMPLEXIPC|GUEST_AGENT_RECEIVE_XFTP_PASS|" else "SIMPLEXIPC|GUEST_AGENT_RECEIVE_FILE_PASS|") <> show size)
                else do
                  let body = xftpReadyName <> " (exceeds the 1 MiB attachment limit)"
                      payload = B.pack
                        (1 : word64LE xftpReadyHandle <> word64LE xftpReadyMsgId
                          <> word16LE (fromIntegral (B.length body))) <> body <> B.pack (word16LE xftpReadyPart)
                  B.useAsCStringLen payload $ \(ptr, length_) ->
                    simplexIpcCompleteBytes requestId 1 (castPtr ptr) (fromIntegral length_)
            Nothing -> deliverRest connectionIds remaining
#else
          deliverRest connectionIds remaining
#endif
        deliverRest connectionIds remaining = do
          receipt <- atomicModifyIORef' deliveryReceipts $ \pending -> case pending of
            [] -> ([], Nothing)
            item : rest -> (rest, Just item)
          case receipt of
            Just (handle, messageId) -> do
              let payload = B.pack (2 : word64LE handle <> word64LE messageId)
              B.useAsCStringLen payload $ \(bytes, length_) ->
                simplexIpcCompleteBytes requestId 1 (castPtr bytes) (fromIntegral length_)
              putStrLn "SIMPLEXIPC|GUEST_AGENT_DELIVERY_PASS"
            Nothing -> do
              queued <- atomicModifyIORef' pendingPieces $ \waiting -> case waiting of
                [] -> ([], Nothing)
                item : rest -> (rest, Just item)
              case queued of
                Just (connectionId, meta, part, body)
                  | connectionId `elem` connectionIds ->
                      deliverText connectionIds connectionId meta part body remaining
                  | remaining > 1 -> deliver connectionIds (remaining - 1)
                  | otherwise -> simplexIpcComplete requestId 2 0
                Nothing -> deliverQueued connectionIds remaining
        deliverQueued connectionIds remaining = do
          next <- atomicModifyIORef' eventBacklog $ \events -> case events of
            [] -> ([], Nothing)
            event : rest -> (rest, Just event)
          case next of
            Just (_, connectionId, AgentProtocol.AEvt AgentProtocol.SAEConn
                (AgentProtocol.MSG meta _ body))
              | connectionId `elem` connectionIds -> do
                  let pieces = splitChatBatch body
                  case pieces of
                    [] -> do
                      _ <- runExceptT $ Agent.ackMessage client connectionId (fromIntegral (fst (AgentProtocol.recipient meta))) Nothing
                      if remaining <= 1
                        then simplexIpcComplete requestId 2 0
                        else deliver connectionIds (remaining - 1)
                    first : rest -> do
                      when (not (null rest)) $ do
                        modifyIORef' pendingPieces (<> [(connectionId, meta, part, piece) | (part, piece) <- zip [1..] rest])
                        putStrLn ("SIMPLEXIPC|GUEST_AGENT_BATCH|" <> show (length pieces))
                      deliverText connectionIds connectionId meta 0 first remaining
            Just _ | remaining > 1 -> deliver connectionIds (remaining - 1)
            _ -> simplexIpcComplete requestId 2 0
        deliverText connectionIds connectionId meta part body remaining = do
          putStrLn "SIMPLEXIPC|GUEST_AGENT_INBOX_EVENT|MSG"
          linked <- readIORef groupLinks
          case find ((== connectionId) . groupLinkConn) linked of
            Just link -> deliverLinked link
            Nothing -> deliverDirect
          where
            handle = stableConnectionHandle connectionId
            messageId = fst (AgentProtocol.recipient meta)
            deliverLinked link = case Aeson.eitherDecodeStrict' body of
              Right (IncomingText text) ->
                deliverMemberText (groupLinkHost link) (groupLinkName link) text
              Right (IncomingForwarded text) ->
                deliverMemberText (groupLinkHost link) B.empty text
              Right (IncomingReact targetMsgId emoji add) ->
                let framed =
                      Text.concat
                        [ Text.pack "\x1e\&R\x1f"
                        , targetMsgId
                        , Text.singleton '\x1f'
                        , emoji
                        ]
                        <> if add then Text.empty else Text.pack "\x1f0"
                 in deliverMemberText (groupLinkHost link) (groupLinkName link) framed
              Right (IncomingImage caption _) ->
                deliverMemberText (groupLinkHost link) (groupLinkName link)
                  (if Text.null caption then "Photo" else caption)
              _ -> ackIntro
            deliverMemberText hostConn namePrefix text = do
              let shown = case TE.decodeUtf8' namePrefix of
                    Right name | not (Text.null name) -> groupSender name text
                    _ -> text
                  textBytes = TE.encodeUtf8 shown
              if B.null textBytes || B.length textBytes > 2048
                then ackIntro
                else do
                  let payload = B.pack
                        (5 : word64LE handle <> word64LE (stableConnectionHandle hostConn)
                          <> word64LE (fromIntegral messageId)
                          <> word16LE (fromIntegral (B.length textBytes))) <> textBytes <> B.pack (word16LE part)
                  B.useAsCStringLen payload $ \(bytes, length_) ->
                    simplexIpcCompleteBytes requestId 1 (castPtr bytes) (fromIntegral length_)
                  putStrLn "SIMPLEXIPC|GUEST_AGENT_GROUP_MEMBER_PASS"
            deliverDirect = case Aeson.eitherDecodeStrict' body of
              Right (IncomingProfile info) -> do
                savePeerProfile store connectionId info
                putStrLn "SIMPLEXIPC|GUEST_AGENT_PEER_INFO_MSG"
                ackIntro
              Right (IncomingText text)
                | B.length (TE.encodeUtf8 text) <= 2048 && not (B.null (TE.encodeUtf8 text)) ->
                    finishText handle text "SIMPLEXIPC|GUEST_AGENT_RECEIVE_MESSAGE_PASS"
              Right (IncomingForwarded text)
                | B.length (TE.encodeUtf8 text) <= 2048 && not (B.null (TE.encodeUtf8 text)) ->
                    finishText handle text "SIMPLEXIPC|GUEST_AGENT_GROUP_MESSAGE_PASS"
              Right (IncomingReact targetMsgId emoji add) -> do
                let framed =
                      Text.concat
                        [ Text.pack "\x1e\&R\x1f"
                        , targetMsgId
                        , Text.singleton '\x1f'
                        , emoji
                        ]
                        <> if add then Text.empty else Text.pack "\x1f0"
                finishText handle framed "SIMPLEXIPC|GUEST_AGENT_RECEIVE_REACT_PASS"
              Right (IncomingImage caption bytes) -> do
                writeIORef imageBytes bytes
                let captionBytes = B.take 128 (TE.encodeUtf8 caption)
                    first = B.take 3000 bytes
                    payload = B.pack
                      (3 : word64LE handle <> word64LE (fromIntegral messageId)
                        <> word16LE (fromIntegral (B.length captionBytes)))
                      <> captionBytes
                      <> B.pack (word16LE (fromIntegral (B.length bytes))
                        <> word16LE (fromIntegral (B.length first)))
                      <> first <> B.pack (word16LE part)
                B.useAsCStringLen payload $ \(ptr, length_) ->
                  simplexIpcCompleteBytes requestId 1 (castPtr ptr) (fromIntegral length_)
                putStrLn ("SIMPLEXIPC|GUEST_AGENT_RECEIVE_IMAGE_PASS|" <> show (B.length bytes))
#ifdef SIMPLEX_PUBLIC_SMP
              Right (IncomingFileDescr sharedId descrText True) -> do
                offer <- loadAttachmentOffer store connectionId sharedId
                let (nameBytes, rowId, rowPart) = case offer of
                      Just (name, offeredId, offeredPart) -> (name, fromIntegral offeredId, fromIntegral offeredPart)
                      Nothing -> ("File", fromIntegral messageId, part)
                queueXftpReceive store client userId pendingXftpRcv nameBytes connectionId handle rowId rowPart descrText
                ackIntro
              Right (IncomingFileDescr _ _ False) -> ackIntro
              Right (IncomingFileOffer sharedId name mDescr) -> do
                let nameBytes = TE.encodeUtf8 (Text.take 80 name)
                forM_ sharedId $ \shared ->
                  saveAttachmentOffer store connectionId shared nameBytes
                    (fromIntegral messageId) (fromIntegral part)
                case mDescr of
                  Just (descrText, True) | not (Text.null descrText) ->
                    queueXftpReceive store client userId pendingXftpRcv nameBytes connectionId handle (fromIntegral messageId) part descrText
                  _ -> pure ()
                finishText handle name "SIMPLEXIPC|GUEST_AGENT_RECEIVE_FILE_NAME"
#endif
              Right (IncomingGroupInvite groupName memberId uri) -> do
                let nameBytes = B.take 64 (TE.encodeUtf8 groupName)
                    uriBytes = TE.encodeUtf8 uri
                    memberBytes = TE.encodeUtf8 memberId
                if B.null nameBytes || B.null memberBytes || B.null uriBytes || B.length uriBytes > 2048
                  then ackIntro
                  else do
                    saveGroupInvite store groupInvites connectionId uriBytes memberBytes (sanitizeGroupName nameBytes)
                    let payload = B.pack
                          (4 : word64LE handle <> word64LE (fromIntegral messageId)
                            <> word16LE (fromIntegral (B.length nameBytes)))
                          <> nameBytes
                          <> B.pack (word16LE (fromIntegral (B.length uriBytes)))
                          <> uriBytes <> B.pack (word16LE part)
                    B.useAsCStringLen payload $ \(ptr, length_) ->
                      simplexIpcCompleteBytes requestId 1 (castPtr ptr) (fromIntegral length_)
                    putStrLn "SIMPLEXIPC|GUEST_AGENT_GROUP_INVITE_PASS"
              Right (IncomingMemberIntro memberId memberName) ->
                introduceMember memberId memberName
              Right (IncomingMemberForward memberId memberName uri) ->
                joinForwardedMember memberId memberName uri
              _ -> ackIntro
            finishText replyHandle text label = do
              let textBytes = TE.encodeUtf8 text
                  payload = B.pack
                    (1 : word64LE replyHandle <> word64LE (fromIntegral messageId)
                      <> word16LE (fromIntegral (B.length textBytes))) <> textBytes <> B.pack (word16LE part)
              B.useAsCStringLen payload $ \(bytes, length_) ->
                simplexIpcCompleteBytes requestId 1 (castPtr bytes) (fromIntegral length_)
              putStrLn label
            ackIntro = do
              more <- batchHasMore pendingPieces connectionId (fromIntegral messageId)
              when (not more) $ void $ runExceptT $
                Agent.ackMessage client connectionId (fromIntegral messageId) Nothing
              putStrLn "SIMPLEXIPC|GUEST_AGENT_SKIP_NON_TEXT"
              if remaining <= 1
                then simplexIpcComplete requestId 2 0
                else deliver connectionIds (remaining - 1)
            introduceMember memberId memberName = do
              known <- readIORef groupLinks
              let memberBytes = TE.encodeUtf8 memberId
                  already = any (\link -> groupLinkHost link == connectionId && groupLinkMember link == memberBytes) known
              if already || length known >= 8 || B.null memberBytes
                then ackIntro
                else do
                  created <- runExceptT $ Agent.createConnection client NRMInteractive userId True True
                    AgentProtocol.SCMInvitation Nothing Nothing IKPQOff SMSubscribe
                  case created of
                    Right (newConn, AgentProtocol.CCLink requestUri _) -> do
                      let uri = strEncode requestUri
                          invite = BL.toStrict . Aeson.encode $ Aeson.object
                            [ "v" .= ("1-19" :: String)
                            , "event" .= ("x.grp.mem.inv" :: String)
                            , "params" .= Aeson.object
                                [ "memberId" .= memberId
                                , "memberIntro" .= Aeson.object ["groupConnReq" .= TE.decodeUtf8 uri]
                                ]
                            ]
                      sent <- runExceptT $ Agent.sendMessage client connectionId PQEncOn noMsgFlags invite
                      case sent of
                        Right _ -> do
                          saveGroupLink store groupLinks newConn connectionId memberBytes (TE.encodeUtf8 memberName)
                          finishText handle (groupSender memberName "joined the group") "SIMPLEXIPC|GUEST_AGENT_GROUP_INTRO_PASS"
                        Left _ -> do
                          void $ runExceptT $ Agent.deleteConnection client NRMInteractive newConn
                          ackIntro
                    _ -> ackIntro
            joinForwardedMember memberId memberName uriText = do
              selves <- readIORef groupSelves
              known <- readIORef groupLinks
              let memberBytes = TE.encodeUtf8 memberId
                  uriBytes = TE.encodeUtf8 uriText
                  already = any (\link -> groupLinkHost link == connectionId && groupLinkMember link == memberBytes) known
              case (already || length known >= 8 || B.null memberBytes, find ((== connectionId) . groupSelfHost) selves, strDecode uriBytes) of
                (False, Just self, Right (AgentProtocol.ACR _ requestUri)) -> do
                  prepared <- runExceptT $ Agent.prepareConnectionToJoin client userId True requestUri PQSupportOn
                  case prepared of
                    Right newConn -> do
                      shown <- sanitizeProfileName <$> readIORef guestProfileName
                      let hello = BL.toStrict . Aeson.encode $ Aeson.object
                            [ "v" .= ("1-19" :: String)
                            , "event" .= ("x.grp.mem.info" :: String)
                            , "params" .= Aeson.object
                                [ "memberId" .= TE.decodeUtf8 (groupSelfMember self)
                                , "profile" .= Aeson.object
                                    [ "displayName" .= TE.decodeUtf8 shown
                                    , "fullName" .= ("" :: String)
                                    ]
                                ]
                            ]
                      joined <- runExceptT $ Agent.joinConnection client NRMInteractive userId
                        newConn True requestUri hello PQSupportOn SMSubscribe
                      case joined of
                        Right _ -> do
                          saveGroupLink store groupLinks newConn connectionId memberBytes (TE.encodeUtf8 memberName)
                          finishText handle (groupSender memberName "joined the group") "SIMPLEXIPC|GUEST_AGENT_GROUP_FORWARD_PASS"
                        Left _ -> do
                          void $ runExceptT $ Agent.deleteConnection client NRMInteractive newConn
                          ackIntro
                    _ -> ackIntro
                _ -> ackIntro

#ifdef SIMPLEX_AGENT_SERVICE_MODE
batchHasMore
  :: IORef [(AgentProtocol.ConnId, AgentProtocol.MsgMeta, Word16, B.ByteString)]
  -> AgentProtocol.ConnId -> Word64 -> IO Bool
batchHasMore pending connectionId msgId =
  any (\(conn, meta, _, _) -> conn == connectionId && fromIntegral (fst (AgentProtocol.recipient meta)) == msgId)
    <$> readIORef pending
#endif

data IncomingBody
  = IncomingText Text.Text
  | IncomingForwarded Text.Text
  | IncomingImage Text.Text B.ByteString
  | IncomingFileDescr Text.Text Text.Text Bool
  | IncomingFileOffer (Maybe Text.Text) Text.Text (Maybe (Text.Text, Bool))
  | IncomingGroupInvite Text.Text Text.Text Text.Text
  | IncomingMemberIntro Text.Text Text.Text
  | IncomingMemberForward Text.Text Text.Text Text.Text
  | IncomingProfile B.ByteString
  | IncomingReact Text.Text Text.Text Bool

instance FromJSON IncomingBody where
  parseJSON = withObject "SimpleX event" $ \event -> do
    name <- event .: "event" :: Parser Text.Text
    params <- event .: "params" :: Parser Aeson.Object
    case name of
      "x.msg.new" -> do
        sharedId <- event .:? "msgId"
        parseContent sharedId params
      "x.msg.react" -> parseReact params
      "x.msg.file.descr" -> parseFileDescr params
      "x.grp.inv" -> parseGroupInvite params
      "x.grp.msg.forward" -> parseGroupForward params
      "x.grp.mem.intro" -> parseMemberIntro params
      "x.grp.mem.fwd" -> parseMemberForward params
      -- Official apps push portrait updates as x.info on the chat connection.
      "x.info" -> do
        profile <- params .: "profile" :: Parser Aeson.Value
        pure (IncomingProfile (BL.toStrict (Aeson.encode profile)))
      _ -> fail "unsupported SimpleX event"

parseReact :: Aeson.Object -> Parser IncomingBody
parseReact params = do
  msgId <- params .: "msgId" :: Parser Text.Text
  add <- params .:? "add" .!= True
  reaction <- params .: "reaction" :: Parser Aeson.Object
  kind <- reaction .:? "type" .!= ("emoji" :: Text.Text)
  emoji <- reaction .:? "emoji" .!= ("" :: Text.Text)
  if kind == "emoji" && not (Text.null msgId) && not (Text.null emoji)
    then pure (IncomingReact msgId (Text.take 1 emoji) add)
    else fail "unsupported SimpleX reaction"

parseFileDescr :: Aeson.Object -> Parser IncomingBody
parseFileDescr params = do
  msgId <- params .: "msgId" :: Parser Text.Text
  descr <- params .: "fileDescr" :: Parser Aeson.Object
  descrText <- descr .: "fileDescrText" :: Parser Text.Text
  complete <- descr .:? "fileDescrComplete" .!= False
  pure (IncomingFileDescr msgId descrText complete)

parseContent :: Maybe Text.Text -> Aeson.Object -> Parser IncomingBody
parseContent sharedId params = do
  content <- params .: "content"
  quoted <- params .:? "quote"
  quoteText <- case quoted of
    Nothing -> pure Nothing
    Just quoteObj -> withObject "SimpleX quote" (\fields -> do
      quotedContent <- fields .: "content"
      withObject "quoted content" (\inner -> do
        kind <- inner .:? "type" .!= ("text" :: Text.Text)
        text <- inner .:? "text" .!= ("" :: Text.Text)
        pure $ if kind == "text" && not (Text.null text) then Just text else Nothing
        ) quotedContent) quoteObj
  withObject "Simplex content" (\fields -> do
    kind <- fields .: "type" :: Parser Text.Text
    caption <- fields .:? "text" .!= ""
    let withQuote base = case quoteText of
          Just q -> Text.concat ["↩ ", Text.replace "\n" " " q, " · ", base]
          Nothing -> base
    case kind of
      "text" -> pure (IncomingText (withQuote caption))
      "image" -> do
        image <- fields .: "image" :: Parser Text.Text
        pure (IncomingImage (withQuote caption) (decodeImagePreview image))
      "file" -> do
        offered <- params .:? "file"
        parsed <- case offered of
          Nothing -> pure ("file", Nothing)
          Just obj -> flip (withObject "SimpleX file") obj $ \fileObj -> do
            name <- fileObj .:? "fileName" .!= ("file" :: Text.Text)
            descrObj <- fileObj .:? "fileDescr"
            descr <- case descrObj of
              Nothing -> pure Nothing
              Just descrValue -> flip (withObject "SimpleX file description") descrValue $ \descrFields -> do
                descrText <- descrFields .:? "fileDescrText" .!= ("" :: Text.Text)
                complete <- descrFields .:? "fileDescrComplete" .!= False
                pure (Just (descrText, complete))
            pure (name, descr)
        pure (IncomingFileOffer sharedId (Text.take 80 (fst parsed)) (snd parsed))
      _ -> fail "unsupported SimpleX content") content

parseGroupInvite :: Aeson.Object -> Parser IncomingBody
parseGroupInvite params = do
  inv <- params .: "groupInvitation" :: Parser Aeson.Object
  profile <- inv .: "groupProfile" :: Parser Aeson.Object
  groupName <- profile .: "displayName" :: Parser Text.Text
  invited <- inv .: "invitedMember" :: Parser Aeson.Object
  memberId <- invited .: "memberId" :: Parser Text.Text
  uri <- inv .: "connRequest" :: Parser Text.Text
  pure (IncomingGroupInvite groupName memberId uri)

parseGroupForward :: Aeson.Object -> Parser IncomingBody
parseGroupForward params = do
  sender <- params .:? "memberName" .!= ""
  message <- params .: "msg"
  inner <- case message of
    Aeson.String encoded ->
      case Aeson.eitherDecodeStrict' (TE.encodeUtf8 encoded) of
        Right body -> pure body
        Left err -> fail err
    other -> Aeson.parseJSON other
  case inner of
    IncomingText text -> pure (IncomingForwarded (groupSender sender text))
    IncomingImage caption bytes -> pure (IncomingImage (groupSender sender caption) bytes)
    IncomingForwarded text -> pure (IncomingForwarded (groupSender sender text))
    _ -> fail "unsupported forwarded SimpleX content"

groupSender :: Text.Text -> Text.Text -> Text.Text
groupSender sender text
  | Text.null sender = text
  | otherwise = Text.take 48 sender <> ": " <> text

-- Several chat events can arrive as one JSON array. Hand them out one by one.
splitChatBatch :: B.ByteString -> [B.ByteString]
splitChatBatch body
  | not (B.null body) && B.head body == 91 =
      case Aeson.eitherDecodeStrict' body of
        Right values -> map (BL.toStrict . Aeson.encode) (values :: [Aeson.Value])
        Left _ -> [body]
  | otherwise = [body]

parseMemberIntro :: Aeson.Object -> Parser IncomingBody
parseMemberIntro params = do
  info <- params .: "memberInfo" :: Parser Aeson.Object
  memberId <- info .: "memberId" :: Parser Text.Text
  profile <- info .: "profile" :: Parser Aeson.Object
  memberName <- profile .:? "displayName" .!= ""
  pure (IncomingMemberIntro memberId memberName)

parseMemberForward :: Aeson.Object -> Parser IncomingBody
parseMemberForward params = do
  info <- params .: "memberInfo" :: Parser Aeson.Object
  memberId <- info .: "memberId" :: Parser Text.Text
  profile <- info .: "profile" :: Parser Aeson.Object
  memberName <- profile .:? "displayName" .!= ""
  intro <- params .: "memberIntro" :: Parser Aeson.Object
  uri <- intro .: "groupConnReq" :: Parser Text.Text
  pure (IncomingMemberForward memberId memberName uri)

-- The other app embeds a resized preview, at most about 14 KB of data-URL
-- text. Keep the decoded bytes that fit in the guest buffer.
decodeImagePreview :: Text.Text -> B.ByteString
decodeImagePreview value =
  case Text.breakOn "," value of
    (header, payload)
      | "data:image/" `Text.isPrefixOf` header && not (Text.null payload) ->
          maybe B.empty id (decodeBase64 (TE.encodeUtf8 (Text.drop 1 payload)))
    _ -> B.empty

decodeBase64 :: B.ByteString -> Maybe B.ByteString
decodeBase64 input
  | B.length input > 20000 = Nothing
  | otherwise = go input [] 0 0
  where
    go rest acc bits have
      | have >= 8 =
          let byte = fromIntegral (bits `shiftR` (have - 8))
              bits' = bits .&. ((1 `shiftL` (have - 8)) - 1)
          in go rest (byte : acc) bits' (have - 8)
      | B.null rest || (not (B.null rest) && B.head rest == 61) =
          if (have == 0 || have == 2 || have == 4) && length acc <= 12288
            then Just (B.pack (reverse acc))
            else Nothing
      | otherwise =
          let byte = B.head rest
          in if byte == 10 || byte == 13 || byte == 32
            then go (B.tail rest) acc bits have
            else case base64Value byte of
              Nothing -> Nothing
              Just value -> go (B.tail rest) acc ((bits `shiftL` 6) .|. value) (have + 6)

base64Value :: Word8 -> Maybe Int
base64Value byte
  | byte >= 65 && byte <= 90 = Just (fromIntegral byte - 65)
  | byte >= 97 && byte <= 122 = Just (fromIntegral byte - 71)
  | byte >= 48 && byte <= 57 = Just (fromIntegral byte + 4)
  | byte == 43 = Just 62
  | byte == 47 = Just 63
  | otherwise = Nothing

sendImageChunk :: Word32 -> B.ByteString -> IORef B.ByteString -> IO ()
sendImageChunk requestId request imageBytes
  | B.length request /= 4 = simplexIpcComplete requestId 3 0
  | otherwise = do
      let offset = sum [fromIntegral (B.index request i) `shiftL` (8 * i) | i <- [0..3]]
      bytes <- readIORef imageBytes
      if offset > B.length bytes
        then simplexIpcComplete requestId 3 0
        else do
          let chunk = B.take 3000 (B.drop offset bytes)
          B.useAsCStringLen chunk $ \(ptr, length_) ->
            simplexIpcCompleteBytes requestId 1 (castPtr ptr) (fromIntegral length_)
#endif

#ifdef SIMPLEX_AGENT_SERVICE_MODE
#ifdef SIMPLEX_PUBLIC_SMP
processAvailableAgentEvents :: DBStore -> Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IORef [(Word64, Word64)]
  -> IORef [PendingXftpSend]
  -> IORef [PendingXftpRcv]
  -> IORef (Seq.Seq XftpImageReady) -> IO ()
processAvailableAgentEvents store client backlog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady = do
  -- awaitEventFor parks non-matching events (notably RCVD) in the backlog while
  -- waiting for SENT. Re-ingest those side effects so delivery checks are not
  -- lost; leave MSG bodies for ReceiveNext.
  flushBacklogSideEffects store client backlog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady
  drain 32
#else
processAvailableAgentEvents :: DBStore -> Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IORef [(Word64, Word64)] -> IO ()
processAvailableAgentEvents store client backlog deliveryReceipts = do
  flushBacklogSideEffects store client backlog deliveryReceipts
  drain 32
#endif
  where
    drain 0 = pure ()
    drain remaining = do
      next <- atomically $ tryReadTBQueue (Agent.subQ client)
      case next of
        Nothing -> pure ()
        Just event -> do
#ifdef SIMPLEX_PUBLIC_SMP
          ingestAgentEvent store client backlog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady event
#else
          ingestAgentEvent store client backlog deliveryReceipts event
#endif
          drain (remaining - 1)

#ifdef SIMPLEX_PUBLIC_SMP
flushBacklogSideEffects
  :: DBStore
  -> Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IORef [(Word64, Word64)]
  -> IORef [PendingXftpSend]
  -> IORef [PendingXftpRcv]
  -> IORef (Seq.Seq XftpImageReady)
  -> IO ()
flushBacklogSideEffects store client backlog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady = do
  side <- atomicModifyIORef' backlog $ \events ->
    let (keep, takeSide) = foldr split ([], []) events
     in (keep, takeSide)
  forM_ side $
    ingestAgentEvent store client backlog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady
  where
    split event (keep, side)
      | backlogSideEffect event = (keep, event : side)
      | otherwise = (event : keep, side)
#else
flushBacklogSideEffects
  :: DBStore
  -> Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IORef [(Word64, Word64)]
  -> IO ()
flushBacklogSideEffects store client backlog deliveryReceipts = do
  side <- atomicModifyIORef' backlog $ \events ->
    let (keep, takeSide) = foldr split ([], []) events
     in (keep, takeSide)
  forM_ side $ ingestAgentEvent store client backlog deliveryReceipts
  where
    split event (keep, side)
      | backlogSideEffect event = (keep, event : side)
      | otherwise = (event : keep, side)
#endif

backlogSideEffect :: AgentProtocol.ATransmission -> Bool
backlogSideEffect (_, _, AgentProtocol.AEvt AgentProtocol.SAEConn event) = case event of
  AgentProtocol.RCVD {} -> True
  AgentProtocol.CON {} -> True
  AgentProtocol.CONF {} -> True
  AgentProtocol.INFO {} -> True
  _ -> False
backlogSideEffect (_, _, AgentProtocol.AEvt AgentProtocol.SAESndFile _) = True
backlogSideEffect (_, _, AgentProtocol.AEvt AgentProtocol.SAERcvFile _) = True
backlogSideEffect _ = False

#ifdef SIMPLEX_PUBLIC_SMP
waitForRelay :: DBStore -> Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IORef [(Word64, Word64)]
  -> IORef [PendingXftpSend]
  -> IORef [PendingXftpRcv]
  -> IORef (Seq.Seq XftpImageReady) -> IO ()
waitForRelay store client backlog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady = do
  threadDelay 200000
  processAvailableAgentEvents store client backlog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady
  pokeInbox backlog
  -- XFTP decrypt can finish after an empty ReceiveNext reply; a coalesced
  -- notify is easy to miss, so keep poking while the file is waiting.
  ready <- readIORef xftpImageReady
  when (not (Seq.null ready)) simplexIpcNotify
#else
waitForRelay :: DBStore -> Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IORef [(Word64, Word64)] -> IO ()
waitForRelay store client backlog deliveryReceipts = do
  threadDelay 200000
  processAvailableAgentEvents store client backlog deliveryReceipts
  pokeInbox backlog
#endif

pokeInbox :: IORef [AgentProtocol.ATransmission] -> IO ()
pokeInbox backlog = do
  waiting <- readIORef backlog
  when (any isQueuedMessage waiting) simplexIpcNotify
  where
    isQueuedMessage (_, _, AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.MSG _ _ _)) = True
    isQueuedMessage _ = False

#ifdef SIMPLEX_PUBLIC_SMP
ingestAgentEvent :: DBStore -> Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IORef [(Word64, Word64)]
  -> IORef [PendingXftpSend]
  -> IORef [PendingXftpRcv]
  -> IORef (Seq.Seq XftpImageReady)
  -> AgentProtocol.ATransmission -> IO ()
ingestAgentEvent store client backlog deliveryReceipts pendingXftp pendingXftpRcv xftpImageReady event = case event of
#else
ingestAgentEvent :: DBStore -> Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IORef [(Word64, Word64)]
  -> AgentProtocol.ATransmission -> IO ()
ingestAgentEvent store client backlog deliveryReceipts event = case event of
#endif
  (_, connectionId, AgentProtocol.AEvt AgentProtocol.SAEConn
      (AgentProtocol.CONF confirmationId _ _ info)) -> do
    connections <- withTransaction store AgentStore.getConnIds
    if connectionId `elem` connections
      then do
        hosted <- hostedGroupConn store connectionId
        ownInfo <- if hosted
          then pure groupHostAllowInfo
          else do
            savePeerProfile store connectionId info
            ownProfileInfo
        allowed <- runExceptT $ Agent.allowConnection client connectionId confirmationId ownInfo
        case allowed of
          Left err -> putStrLn ("SIMPLEXIPC|GUEST_AGENT_ALLOW_CONNECTION_FAIL|" <> show err)
          Right () ->
            putStrLn $ if hosted
              then "SIMPLEXIPC|GUEST_AGENT_GROUP_HOST_ALLOW"
              else "SIMPLEXIPC|GUEST_AGENT_ALLOW_CONNECTION_PASS"
      else queue event
  (_, connectionId, AgentProtocol.AEvt AgentProtocol.SAEConn
      (AgentProtocol.INFO _ info)) -> do
    savePeerProfile store connectionId info
    putStrLn "SIMPLEXIPC|GUEST_AGENT_PEER_INFO"
  event'@(_, connectionId, AgentProtocol.AEvt AgentProtocol.SAEConn
      (AgentProtocol.CON _)) -> do
    connections <- withTransaction store AgentStore.getConnIds
    if connectionId `elem` connections
      then do
        subscribed <- runExceptT $ Agent.subscribeConnection client connectionId
        case subscribed of
          Left err -> putStrLn ("SIMPLEXIPC|GUEST_AGENT_SUBSCRIBE_CONNECTION_FAIL|" <> show err)
          Right () -> putStrLn "SIMPLEXIPC|GUEST_AGENT_CONNECTION_READY"
      else queue event'
  (_, connectionId, AgentProtocol.AEvt AgentProtocol.SAEConn
      (AgentProtocol.RCVD meta receipts)) -> do
    acknowledgeBroker connectionId meta
    let handle = stableConnectionHandle connectionId
    forM_ receipts $ \(AgentProtocol.MsgReceipt deliveredId status) ->
      when (status == AgentProtocol.MROk && deliveredId > 0) $ do
        modifyIORef' deliveryReceipts $ \pending ->
          take 16 (pending <> [(handle, fromIntegral deliveredId)])
        simplexIpcNotify
        putStrLn "SIMPLEXIPC|GUEST_AGENT_DELIVERY_QUEUED"
#ifdef SIMPLEX_PUBLIC_SMP
  -- ATransmission is (corrId, entityId, event); XFTP notifies with empty corrId.
  (_, sndFileId, AgentProtocol.AEvt AgentProtocol.SAESndFile
      (AgentProtocol.SFDONE _ recipientFds)) ->
    completePendingXftpSend client backlog pendingXftp sndFileId recipientFds
  (_, sndFileId, AgentProtocol.AEvt AgentProtocol.SAESndFile
      (AgentProtocol.SFERR err)) ->
    putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_SEND_FAIL|" <> show err <> "|" <> show (B.length sndFileId))
  (_, _, AgentProtocol.AEvt AgentProtocol.SAESndFile
      (AgentProtocol.SFPROG sent total)) ->
    putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_PROG|" <> show sent <> "|" <> show total)
  (_, rcvFileId, AgentProtocol.AEvt AgentProtocol.SAERcvFile
      (AgentProtocol.RFDONE path)) -> do
    matched <- atomicModifyIORef' pendingXftpRcv $ \pending ->
      case break ((== rcvFileId) . pendingXftpRcvId) pending of
        (before, found : after) -> (before ++ after, Just found)
        (_, []) -> (pending, Nothing)
    case matched of
      Just PendingXftpRcv {pendingXftpRcvConn, pendingXftpRcvHandle, pendingXftpRcvMsgId, pendingXftpRcvPart, pendingXftpRcvName} -> do
        bytes <- B.readFile path
        -- Official image/file payloads over XFTP are often well above the
        -- inline preview cap; keep a generous guest-side ceiling.
        if B.null bytes || B.length bytes > (8 * 1024 * 1024)
          then putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_RCV_BAD_SIZE|" <> show (B.length bytes))
          else do
            -- ReadImage must keep serving the current JPEG until the shell
            -- asks for the next message, even if another download finishes.
            let completed = XftpImageReady
                  { xftpReadyHandle = pendingXftpRcvHandle
                  , xftpReadyConn = pendingXftpRcvConn
                  , xftpReadyFileId = rcvFileId
                  , xftpReadyMsgId = pendingXftpRcvMsgId
                  , xftpReadyPart = pendingXftpRcvPart
                  , xftpReadyBytes = bytes
                  , xftpReadyName = pendingXftpRcvName
                  }
            atomicModifyIORef' xftpImageReady $ \pending -> (pending Seq.|> completed, ())
            simplexIpcNotify
            putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_RCV_DONE|" <> show (B.length bytes))
      Nothing ->
        putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_RCV_UNMATCHED|" <> show (B.length rcvFileId))
  (_, _, AgentProtocol.AEvt AgentProtocol.SAERcvFile
      (AgentProtocol.RFERR err)) ->
    putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_RCV_FAIL|" <> show err)
  (_, _, AgentProtocol.AEvt AgentProtocol.SAERcvFile
      (AgentProtocol.RFPROG sent total)) ->
    putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_RCV_PROG|" <> show sent <> "|" <> show total)
#endif
  (_, connectionId, AgentProtocol.AEvt AgentProtocol.SAEConn
      (AgentProtocol.REQ invId pqSupport _ info)) -> do
    stored <- loadGuestAddress store
    case stored of
      Just (addressConn, _) | addressConn == connectionId -> do
        users <- withTransaction store AgentStore.getUserIds
        case users of
          [] -> putStrLn "SIMPLEXIPC|GUEST_AGENT_ADDRESS_ACCEPT_FAIL|user"
          userId : _ -> do
            prepared <- runExceptT $
              Agent.prepareConnectionToAccept client userId True invId pqSupport
            case prepared of
              Left err ->
                putStrLn ("SIMPLEXIPC|GUEST_AGENT_ADDRESS_ACCEPT_FAIL|prepare|" <> show err)
              Right newConn -> do
                ownInfo <- ownProfileInfo
                accepted <- runExceptT $
                  Agent.acceptContact client NRMInteractive userId newConn True invId ownInfo pqSupport SMSubscribe
                case accepted of
                  Left err -> do
                    void $ runExceptT $ Agent.deleteConnection client NRMInteractive newConn
                    putStrLn ("SIMPLEXIPC|GUEST_AGENT_ADDRESS_ACCEPT_FAIL|accept|" <> show err)
                  Right _ -> do
                    let name = displayNameBytes info
                    savePeerName store newConn (if B.null name then "Contact" else name)
                    putStrLn "SIMPLEXIPC|GUEST_AGENT_ADDRESS_ACCEPT_PASS"
      _ -> putStrLn "SIMPLEXIPC|GUEST_AGENT_ADDRESS_REQ_OTHER"
  _ -> queue event
  where
    acknowledgeBroker connectionId meta = do
      let messageId = fst (AgentProtocol.recipient meta)
      acknowledged <- runExceptT $ Agent.ackMessage client connectionId messageId Nothing
      case acknowledged of
        Left err -> putStrLn ("SIMPLEXIPC|GUEST_AGENT_ACK_BROKER_FAIL|RCVD|" <> show err)
        Right () -> putStrLn "SIMPLEXIPC|GUEST_AGENT_ACK_BROKER|RCVD"
    queue queued@(_, connectionId, AgentProtocol.AEvt AgentProtocol.SAEConn
        (AgentProtocol.MSG meta _ body)) = do
      -- Official apps send portrait updates as x.info; apply without waiting
      -- for Messages to poll ReceiveNext.
      case Aeson.eitherDecodeStrict' body of
        Right (IncomingProfile info) -> do
          savePeerProfile store connectionId info
          acknowledgeBroker connectionId meta
          putStrLn "SIMPLEXIPC|GUEST_AGENT_PEER_INFO_MSG"
        _ -> do
          putStrLn "SIMPLEXIPC|GUEST_AGENT_EVENT_BACKLOG|MSG"
          modifyIORef' backlog $ \events -> events <> [queued]
          simplexIpcNotify
    queue queued = do
      putStrLn ("SIMPLEXIPC|GUEST_AGENT_EVENT_BACKLOG|OTHER|" <> eventName queued)
      modifyIORef' backlog $ \events -> take 64 (events <> [queued])
      simplexIpcNotify
#endif

#ifdef SIMPLEX_PUBLIC_SMP
-- | Stage the image offer fields and write bytes; delay xftpSendFile until the
-- caller finishes the SMP send (see operation 4).
prepareImageXftpOffer
  :: Text.Text
  -> IO (Maybe (Text.Text, Aeson.Value, B.ByteString, Integer))
prepareImageXftpOffer imageText = do
  let bytes = decodeImagePreview imageText
  if B.null bytes
    then pure Nothing
    else do
      createDirectoryIfMissing True "xftp"
      B.writeFile "xftp/photo.jpg" bytes
      sharedRaw <- getEntropy 12
      let sharedMsgIdText = TE.decodeUtf8 (strEncode sharedRaw)
          fileSize = fromIntegral (B.length bytes) :: Integer
          fileField = Aeson.object
            [ "fileName" .= ("photo.jpg" :: String)
            , "fileSize" .= fileSize
            , "fileDescr" .= Aeson.object
                [ "fileDescrText" .= ("" :: Text.Text)
                , "fileDescrPartNo" .= (0 :: Int)
                , "fileDescrComplete" .= False
                ]
            ]
      pure (Just (sharedMsgIdText, fileField, sharedRaw, fileSize))

startImageXftp
  :: Agent.AgentClient
  -> AgentProtocol.UserId
  -> IORef [PendingXftpSend]
  -> AgentProtocol.ConnId
  -> B.ByteString
  -> Integer
  -> IO ()
-- The XFTP worker's temporary files are volatile on the guest. Keep the
-- validated download description in the encrypted agent DB until Messages
-- confirms its own durable save. A reboot can then restart the download.
restoreXftpReceives
  :: DBStore -> Agent.AgentClient -> AgentProtocol.UserId -> IORef [PendingXftpRcv] -> IO ()
restoreXftpReceives store client userId pending = do
  saved <- withTransaction' store $ \db -> do
    SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_attachment_offers (conn_id BLOB NOT NULL, shared_id BLOB NOT NULL, name BLOB NOT NULL, msg_id INTEGER NOT NULL, part INTEGER NOT NULL, PRIMARY KEY (conn_id, shared_id))"
    SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_xftp_inbox (conn_id BLOB NOT NULL, msg_id INTEGER NOT NULL, part INTEGER NOT NULL, name BLOB NOT NULL, descr BLOB NOT NULL, rcv_id BLOB, PRIMARY KEY (conn_id, msg_id, part))"
    SQL.query_ db "SELECT conn_id, msg_id, part, name, descr, rcv_id FROM guest_xftp_inbox ORDER BY rowid"
      :: IO [(AgentProtocol.ConnId, Int64, Int64, B.ByteString, B.ByteString, Maybe B.ByteString)]
  live <- withTransaction store AgentStore.getConnIds
  forM_ saved $ \(connectionId, messageId, part, name, descr, oldId) -> do
    forM_ oldId (Agent.xftpDeleteRcvFile client)
    if connectionId `elem` live
      then startXftpReceive store client userId pending name connectionId
        (stableConnectionHandle connectionId) (fromIntegral messageId) (fromIntegral part) descr
      else withTransaction' store $ \db ->
        SQL.execute db "DELETE FROM guest_xftp_inbox WHERE conn_id = ? AND msg_id = ? AND part = ?"
          (connectionId, messageId, part)

saveAttachmentOffer :: DBStore -> AgentProtocol.ConnId -> Text.Text -> B.ByteString -> Int64 -> Int64 -> IO ()
saveAttachmentOffer store connectionId sharedId name messageId part = withTransaction' store $ \db ->
  SQL.execute db "INSERT OR IGNORE INTO guest_attachment_offers (conn_id, shared_id, name, msg_id, part) VALUES (?, ?, ?, ?, ?)"
    (connectionId, TE.encodeUtf8 sharedId, name, messageId, part)

loadAttachmentOffer :: DBStore -> AgentProtocol.ConnId -> Text.Text -> IO (Maybe (B.ByteString, Int64, Int64))
loadAttachmentOffer store connectionId sharedId = withTransaction' store $ \db -> do
  offers <- SQL.query db "SELECT name, msg_id, part FROM guest_attachment_offers WHERE conn_id = ? AND shared_id = ?"
    (connectionId, TE.encodeUtf8 sharedId)
  pure $ case offers of
    offer : _ -> Just offer
    [] -> Nothing

queueXftpReceive
  :: DBStore -> Agent.AgentClient -> AgentProtocol.UserId -> IORef [PendingXftpRcv]
  -> B.ByteString -> AgentProtocol.ConnId -> Word64 -> Word64 -> Word16 -> Text.Text -> IO ()
queueXftpReceive store client userId pending name connectionId handle messageId part descrText =
  startXftpReceive store client userId pending name connectionId handle messageId part (TE.encodeUtf8 descrText)

startXftpReceive
  :: DBStore -> Agent.AgentClient -> AgentProtocol.UserId -> IORef [PendingXftpRcv]
  -> B.ByteString -> AgentProtocol.ConnId -> Word64 -> Word64 -> Word16 -> B.ByteString -> IO ()
startXftpReceive store client userId pending name connectionId handle messageId part descr =
  case strDecode descr of
    Right (vfd :: ValidFileDescription 'FRecipient) -> do
      let msg = fromIntegral messageId :: Int64
          partId = fromIntegral part :: Int64
          alreadyPending item = pendingXftpRcvConn item == connectionId
            && pendingXftpRcvMsgId item == messageId && pendingXftpRcvPart item == part
      waiting <- readIORef pending
      when (not (any alreadyPending waiting)) $ do
        withTransaction' store $ \db ->
          SQL.execute db "INSERT OR IGNORE INTO guest_xftp_inbox (conn_id, msg_id, part, name, descr) VALUES (?, ?, ?, ?, ?)"
            (connectionId, msg, partId, name, descr)
        started <- runExceptT $ Agent.xftpReceiveFile client userId vfd Nothing True
        case started of
          Left err ->
            putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_RCV_START_FAIL|" <> show err)
          Right rcvId -> do
            withTransaction' store $ \db ->
              SQL.execute db "UPDATE guest_xftp_inbox SET rcv_id = ? WHERE conn_id = ? AND msg_id = ? AND part = ?"
                (rcvId, connectionId, msg, partId)
            modifyIORef' pending $ \waiting' ->
              PendingXftpRcv rcvId connectionId handle messageId part name : waiting'
            putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_RCV_START|" <> show (B.length rcvId))
    Left err ->
      putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_RCV_PARSE_FAIL|" <> err)


startImageXftp client userId pendingXftp connectionId sharedRaw fileSize =
  startXftpFile client userId pendingXftp connectionId sharedRaw fileSize "xftp/photo.jpg"

startXftpFile client userId pendingXftp connectionId sharedRaw fileSize path = do
  -- One recipient key is enough for a 1:1 / group member photo; fewer replica
  -- creates means fewer serialized ephemeral XFTP connects on the single slot.
  started <- runExceptT $ Agent.xftpSendFile client userId (plain path) 1
  case started of
    Left err ->
      putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_START_FAIL|" <> show err)
    Right sndFileId -> do
      modifyIORef' pendingXftp $ \pending ->
        take 8 (PendingXftpSend sndFileId connectionId sharedRaw : pending)
      putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_START|" <> show (B.length sndFileId) <> "|" <> show fileSize)

completePendingXftpSend
  :: Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IORef [PendingXftpSend]
  -> B.ByteString
  -> [ValidFileDescription 'FRecipient]
  -> IO ()
completePendingXftpSend _client _backlog pendingXftp sndFileId recipientFds = do
  matched <- atomicModifyIORef' pendingXftp $ \pending ->
    case break ((== sndFileId) . pendingXftpSndId) pending of
      (before, found : after) -> (before ++ after, Just found)
      (_, []) -> (pending, Nothing)
  case (matched, recipientFds) of
    (Just PendingXftpSend {pendingXftpConn, pendingXftpSharedMsg}, rfd : _) -> do
      -- Do not send while the XFTP TLS session still owns slot 1; that races
      -- the secondary SMP reconnect and drops the YAML descr. Flush later.
      modifyIORef' deferredXftpDescrs $ \pending ->
        take 4 (PendingXftpDescr pendingXftpConn pendingXftpSharedMsg rfd : pending)
      putStrLn "SIMPLEXIPC|GUEST_AGENT_XFTP_DESCR_DEFERRED"
    _ -> putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_DONE_UNMATCHED|" <> show (B.length sndFileId))

flushDeferredXftpDescrs
  :: Agent.AgentClient
  -> IORef [AgentProtocol.ATransmission]
  -> IO ()
flushDeferredXftpDescrs client backlog = do
  free <- tryAcquireEphemeralGate
  when free $ do
    pending <- atomicModifyIORef' deferredXftpDescrs $ \xs -> ([], xs)
    forM_ pending $ \PendingXftpDescr {pendingDescrConn, pendingDescrShared, pendingDescrRfd} -> do
      let descrText = TE.decodeUtf8 (strEncode pendingDescrRfd)
          body = BL.toStrict . Aeson.encode $ Aeson.object
            [ "v" .= ("1-19" :: String)
            , "event" .= ("x.msg.file.descr" :: String)
            , "params" .= Aeson.object
                [ "msgId" .= TE.decodeUtf8 (strEncode pendingDescrShared)
                , "fileDescr" .= Aeson.object
                    [ "fileDescrText" .= descrText
                    , "fileDescrPartNo" .= (1 :: Int)
                    , "fileDescrComplete" .= True
                    ]
                ]
            ]
      let attempt n = do
            sent <- runExceptT $ Agent.sendMessage client pendingDescrConn PQEncOn noMsgFlags body
            case sent of
              Left err
                | n < 8 -> do
                    putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_DESCR_RETRY|" <> show n <> "|" <> show err)
                    threadDelay 1000000
                    attempt (n + 1)
                | otherwise ->
                    putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_DESCR_FAIL|" <> show err)
              Right (descrMsgId, _) -> do
                putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_DESCR_QUEUED|" <> show descrMsgId)
                waited <- runExceptT $
                  awaitAgentEventFor 20000000 backlog client pendingDescrConn $
                    \(_, connId, evt) ->
                      if connId == pendingDescrConn
                        then case evt of
                          AgentProtocol.AEvt AgentProtocol.SAEConn
                            (AgentProtocol.SENT mid _)
                              | mid == descrMsgId -> Just ()
                          _ -> Nothing
                        else Nothing
                case waited of
                  Right () -> putStrLn "SIMPLEXIPC|GUEST_AGENT_XFTP_SEND_PASS"
                  Left err
                    | n < 8 -> do
                        putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_DESCR_SENT_RETRY|" <> show n <> "|" <> show err)
                        threadDelay 1000000
                        attempt (n + 1)
                    | otherwise ->
                        putStrLn ("SIMPLEXIPC|GUEST_AGENT_XFTP_DESCR_FAIL|" <> show err)
      attempt (0 :: Int)
#endif

#ifdef SIMPLEX_AGENT_SERVICE_MODE
decodeInvitationPayload :: B.ByteString -> Maybe B.ByteString
decodeInvitationPayload payload
  | B.length payload < 3 = Nothing
  | invitationLength == 0 || invitationLength > 2048 = Nothing
  | B.length payload /= invitationLength + 2 = Nothing
  | not (B.all (< 128) invitation) = Nothing
  | otherwise = Just invitation
  where
    invitationLength = fromIntegral (B.index payload 0)
      + (fromIntegral (B.index payload 1) `shiftL` 8)
    invitation = B.drop 2 payload
#endif

#ifdef SIMPLEX_AGENT_SERVICE_MODE
-- Slint wire framing (RS=\x1e, US=\x1f):
--   Q US {0|1} US {quote} US {text}  → x.msg.new + params.quote
--   F US {text}                      → x.msg.new + params.forward
--   E US {msgId} US {text}           → x.msg.update
--   D US {msgId}                     → x.msg.del
--   R US {msgId} US {emoji} [US 0]   → x.msg.react (optional 0 = remove)
data SendWire
  = SendNew Text.Text (Maybe Aeson.Value) Bool
  | SendEdit Text.Text Text.Text
  | SendDelete Text.Text
  | SendReact Text.Text Text.Text Bool
  | SendFile Text.Text Text.Text

parseSendWire :: Text.Text -> SendWire
parseSendWire raw
  | Text.isPrefixOf editMark raw =
      case Text.splitOn sep (Text.drop (Text.length editMark) raw) of
        (msgId : rest) | not (Text.null msgId) && not (null rest) ->
          SendEdit msgId (Text.intercalate sep rest)
        _ -> SendNew raw Nothing False
  | Text.isPrefixOf delMark raw =
      let msgId = Text.drop (Text.length delMark) raw
      in if Text.null msgId then SendNew raw Nothing False else SendDelete msgId
  | Text.isPrefixOf reactMark raw =
      case Text.splitOn sep (Text.drop (Text.length reactMark) raw) of
        (msgId : emoji : rest)
          | not (Text.null msgId) && not (Text.null emoji) ->
              let add = case rest of
                    ("0" : _) -> False
                    _ -> True
               in SendReact msgId (Text.take 1 emoji) add
        _ -> SendNew raw Nothing False
  | Text.isPrefixOf fwdMark raw =
      SendNew (Text.drop (Text.length fwdMark) raw) Nothing True
  | Text.isPrefixOf fileMark raw =
      case Text.splitOn sep (Text.drop (Text.length fileMark) raw) of
        (name : rest) | not (Text.null name) && not (null rest) ->
          SendFile (Text.take 40 name) (Text.take 400 (Text.intercalate sep rest))
        _ -> SendNew raw Nothing False
  | otherwise =
      let (text, quote) = splitQuotedSend raw
      in SendNew text quote False
  where
    -- \& ends the hex escape. \x1eE would be one character, U+01EE.
    editMark = Text.pack "\x1e\&E\x1f"
    delMark = Text.pack "\x1e\&D\x1f"
    reactMark = Text.pack "\x1e\&R\x1f"
    fwdMark = Text.pack "\x1e\&F\x1f"
    fileMark = Text.pack "\x1e\&L\x1f"
    sep = Text.singleton '\x1f'

-- Slint frames replies as: RS Q US {0|1} US {quote} US {text}
splitQuotedSend :: Text.Text -> (Text.Text, Maybe Aeson.Value)
splitQuotedSend raw
  | Text.isPrefixOf mark raw =
      case Text.splitOn sep (Text.drop (Text.length mark) raw) of
        (sentFlag : quote : rest) | not (Text.null quote) ->
          let body = Text.intercalate sep rest
              sent = sentFlag == "1"
              quoteObj = Aeson.object
                [ "content" .= Aeson.object
                    [ "type" .= ("text" :: String)
                    , "text" .= Text.take 120 quote
                    ]
                , "msgRef" .= Aeson.object
                    [ "sent" .= sent
                    , "sentAt" .= ("1970-01-01T00:00:01.000000001Z" :: String)
                    ]
                ]
          in (body, Just quoteObj)
        _ -> (raw, Nothing)
  | otherwise = (raw, Nothing)
  where
    mark = Text.pack "\x1e\&Q\x1f"
    sep = Text.singleton '\x1f'

completeAgentSend :: Word32 -> Int64 -> Text.Text -> IO ()
completeAgentSend requestId messageId sharedMsgId = do
  let sid = TE.encodeUtf8 sharedMsgId
      payload =
        B.pack (word64LE (fromIntegral messageId))
          <> B.pack (word16LE (fromIntegral (B.length sid)))
          <> sid
  B.useAsCStringLen payload $ \(bytes, length_) ->
    simplexIpcCompleteBytes requestId 1 (castPtr bytes) (fromIntegral length_)
  putStrLn
    ( if Text.null sharedMsgId
        then "SIMPLEXIPC|GUEST_AGENT_SEND_MESSAGE_PASS"
        else "SIMPLEXIPC|GUEST_AGENT_SEND_MESSAGE_PASS|MSGID"
    )

agentSendToTargets
  :: Word32
  -> Agent.AgentClient
  -> AgentProtocol.ConnId
  -> IORef [GroupSelf]
  -> IORef [GroupLink]
  -> B.ByteString
  -> Text.Text
  -> IO (Maybe Int64)
agentSendToTargets requestId client connectionId groupSelves groupLinks body sharedMsgId = do
  targets <- groupSendTargets groupSelves groupLinks connectionId
  when (length targets > 1) $
    putStrLn ("SIMPLEXIPC|GUEST_AGENT_GROUP_SEND|" <> show (length targets))
  let sendToGroup [] (Just messageId) _ = do
        completeAgentSend requestId messageId sharedMsgId
        pure (Just messageId)
      sendToGroup [] Nothing _ = do
        simplexIpcComplete requestId 5 0
        pure Nothing
      sendToGroup (target : rest) messageId hostSend = do
        sent <- runExceptT $ Agent.sendMessage client target PQEncOn noMsgFlags body
        case sent of
          Left sendError
            | hostSend -> do
                simplexIpcComplete requestId 5 0
                putStrLn ("SIMPLEXIPC|GUEST_AGENT_SEND_MESSAGE_FAIL|" <> show sendError)
                pure Nothing
            | otherwise -> do
                putStrLn ("SIMPLEXIPC|GUEST_AGENT_GROUP_SEND_SKIP|" <> show sendError)
                sendToGroup rest messageId False
          Right (messageId', _)
            | hostSend -> sendToGroup rest (Just messageId') False
            | otherwise -> sendToGroup rest messageId False
  sendToGroup targets Nothing True

decodeCreateGroupPayload :: B.ByteString -> Maybe (Word64, B.ByteString)
decodeCreateGroupPayload payload
  | B.length payload < 10 = Nothing
  | nameLength == 0 || nameLength > 32 = Nothing
  | B.length payload /= 10 + nameLength = Nothing
  | otherwise = Just (readWord64LE (B.take 8 payload), B.drop 10 payload)
  where
    nameLength = fromIntegral (B.index payload 8)
      + (fromIntegral (B.index payload 9) `shiftL` 8)

decodeSendMessagePayload :: B.ByteString -> Maybe (Word64, B.ByteString)
decodeSendMessagePayload payload
  | B.length payload < 10 = Nothing
  | bodyLength == 0 || bodyLength > 2048 = Nothing
  | B.length payload /= 10 + bodyLength = Nothing
  | otherwise = Just (handle, B.drop 10 payload)
  where
    handle = readWord64LE (B.take 8 payload)
    bodyLength = fromIntegral (B.index payload 8)
      + (fromIntegral (B.index payload 9) `shiftL` 8)

decodeAckPayload :: B.ByteString -> Maybe (Word64, Word64)
decodeAckPayload payload
  | B.length payload /= 16 = Nothing
  | otherwise = Just (readWord64LE (B.take 8 payload), readWord64LE (B.drop 8 payload))

decodePeerImagePayload :: B.ByteString -> Maybe (Word64, Int)
decodePeerImagePayload payload
  | B.length payload /= 10 = Nothing
  | otherwise =
      let handle = foldl' (\acc index -> acc + (fromIntegral (B.index payload index) `shiftL` (8 * index))) 0 [0 .. 7]
          offset = fromIntegral (B.index payload 8) + (fromIntegral (B.index payload 9) `shiftL` 8)
      in Just (handle, offset)

decodeDeletePayload :: B.ByteString -> Maybe Word64
decodeDeletePayload payload
  | B.length payload /= 8 = Nothing
  | otherwise = Just (readWord64LE payload)

readWord64LE :: B.ByteString -> Word64
readWord64LE bytes = B.foldr' (\byte value -> (value `shiftL` 8) .|. fromIntegral byte) 0 bytes
#endif

#ifdef SIMPLEX_AGENT_SERVICE_MODE
-- The peer's profile is stored with the accepted confirmation. Return the
-- first connection that has a short display name.
firstPeerName :: DBStore -> [AgentProtocol.ConnId] -> IO (Maybe (AgentProtocol.ConnId, B.ByteString))
firstPeerName _ [] = pure Nothing
firstPeerName store (connectionId : rest) = do
  loaded <- withTransaction store (`AgentStore.getAcceptedConfirmation` connectionId)
  case loaded of
    Right accepted ->
      case senderConf accepted of
        AgentProtocol.SMPConfirmation _ _ info _ _ ->
          case displayNameBytes info of
            name | not (B.null name) -> pure (Just (connectionId, name))
            _ -> firstPeerName store rest
    Left _ -> firstPeerName store rest

rememberedChats :: DBStore -> [AgentProtocol.ConnId] -> IO [(AgentProtocol.ConnId, B.ByteString)]
rememberedChats store connectionIds = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_peer_names (conn_id BLOB PRIMARY KEY, display_name BLOB NOT NULL)"
  saved <- SQL.query_ db "SELECT conn_id, display_name FROM guest_peer_names ORDER BY rowid DESC"
  pure $ take 48 $ filter matches saved
  where
    matches (connectionId, name) =
      connectionId `elem` connectionIds && not (B.null name) && B.length name <= 32 && B.all peerNameChar name
    peerNameChar byte = byte == 32 || (byte >= 48 && byte <= 57) || (byte >= 65 && byte <= 90)
      || (byte >= 97 && byte <= 122) || byte == 45 || byte == 46 || byte == 95

savePeerName :: DBStore -> AgentProtocol.ConnId -> B.ByteString -> IO ()
savePeerName _ _ name | B.null name = pure ()
savePeerName store connectionId name = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_peer_names (conn_id BLOB PRIMARY KEY, display_name BLOB NOT NULL)"
  SQL.execute db "INSERT OR REPLACE INTO guest_peer_names (conn_id, display_name) VALUES (?, ?)" (connectionId, name)
  putStrLn ("SIMPLEXIPC|GUEST_AGENT_PEER_NAME_SAVED|" <> show (B.length name))

savePeerImage :: DBStore -> AgentProtocol.ConnId -> B.ByteString -> IO ()
savePeerImage _ _ bytes | B.null bytes || B.length bytes > 12288 = pure ()
savePeerImage store connectionId bytes = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_peer_images (conn_id BLOB PRIMARY KEY, jpeg BLOB NOT NULL)"
  SQL.execute db "INSERT OR REPLACE INTO guest_peer_images (conn_id, jpeg) VALUES (?, ?)" (connectionId, bytes)
  putStrLn ("SIMPLEXIPC|GUEST_AGENT_PEER_IMAGE_SAVED|" <> show (B.length bytes))

loadPeerImage :: DBStore -> AgentProtocol.ConnId -> IO B.ByteString
loadPeerImage store connectionId = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_peer_images (conn_id BLOB PRIMARY KEY, jpeg BLOB NOT NULL)"
  rows <- SQL.query db "SELECT jpeg FROM guest_peer_images WHERE conn_id = ?" (SQL.Only connectionId)
  pure $ case rows of
    (SQL.Only bytes) : _ -> bytes
    _ -> B.empty

peerPhotoMask :: DBStore -> [(AgentProtocol.ConnId, B.ByteString)] -> IO Word16
peerPhotoMask store chats = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_peer_images (conn_id BLOB PRIMARY KEY, jpeg BLOB NOT NULL)"
  foldl'
    (\bits (index, (connectionId, _)) -> do
        bits' <- bits
        rows <- SQL.query db "SELECT 1 FROM guest_peer_images WHERE conn_id = ? LIMIT 1" (SQL.Only connectionId) :: IO [SQL.Only Int]
        pure $ if null rows then bits' else bits' .|. (1 `shiftL` index))
    (pure 0)
    (zip [0 :: Int ..] chats)

savePeerProfile :: DBStore -> AgentProtocol.ConnId -> B.ByteString -> IO ()
savePeerProfile store connectionId info = do
  savePeerName store connectionId (displayNameBytes info)
  savePeerImage store connectionId (profileImageBytes info)

-- Existing contacts already have the portrait on the accepted confirmation.
-- Seed the avatar table once so the chat list can show it after reboot.
seedPeerImages :: DBStore -> [AgentProtocol.ConnId] -> IO ()
seedPeerImages store connectionIds = forM_ connectionIds $ \connectionId -> do
  existing <- loadPeerImage store connectionId
  when (B.null existing) $ do
    loaded <- withTransaction store (`AgentStore.getAcceptedConfirmation` connectionId)
    case loaded of
      Right accepted -> case senderConf accepted of
        AgentProtocol.SMPConfirmation _ _ info _ _ ->
          savePeerImage store connectionId (profileImageBytes info)
      Left _ -> pure ()

data GroupInvite = GroupInvite
  { groupInviteConn :: AgentProtocol.ConnId
  , groupInviteUri :: B.ByteString
  , groupInviteMember :: B.ByteString
  , groupInviteName :: B.ByteString
  }

loadGroupInvites :: DBStore -> IO [GroupInvite]
loadGroupInvites store = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_group_invites (conn_id BLOB PRIMARY KEY, member_id BLOB NOT NULL, group_name BLOB NOT NULL, conn_uri BLOB NOT NULL)"
  rows <- SQL.query_ db "SELECT conn_id, member_id, group_name, conn_uri FROM guest_group_invites ORDER BY rowid DESC"
  pure [GroupInvite conn member name uri | (conn, member, name, uri) <- rows]

saveGroupInvite :: DBStore -> IORef [GroupInvite] -> AgentProtocol.ConnId -> B.ByteString -> B.ByteString -> B.ByteString -> IO ()
saveGroupInvite store invites connectionId uri member name = do
  withTransaction' store $ \db -> do
    SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_group_invites (conn_id BLOB PRIMARY KEY, member_id BLOB NOT NULL, group_name BLOB NOT NULL, conn_uri BLOB NOT NULL)"
    SQL.execute db "INSERT OR REPLACE INTO guest_group_invites (conn_id, member_id, group_name, conn_uri) VALUES (?, ?, ?, ?)" (connectionId, member, name, uri)
  atomicModifyIORef' invites $ \known ->
    (GroupInvite connectionId uri member name : filter ((/= connectionId) . groupInviteConn) known, ())

forgetGroupInvite :: DBStore -> IORef [GroupInvite] -> B.ByteString -> IO ()
forgetGroupInvite store invites uri = do
  atomicModifyIORef' invites $ \known -> (filter ((/= uri) . groupInviteUri) known, ())
  withTransaction' store $ \db ->
    SQL.execute db "DELETE FROM guest_group_invites WHERE conn_uri = ?" (SQL.Only uri)

data GroupSelf = GroupSelf
  { groupSelfHost :: AgentProtocol.ConnId
  , groupSelfMember :: B.ByteString
  , groupSelfName :: B.ByteString
  }

data GroupLink = GroupLink
  { groupLinkConn :: AgentProtocol.ConnId
  , groupLinkHost :: AgentProtocol.ConnId
  , groupLinkMember :: B.ByteString
  , groupLinkName :: B.ByteString
  }

loadGroupSelves :: DBStore -> IO [GroupSelf]
loadGroupSelves store = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_group_self (host_conn_id BLOB PRIMARY KEY, member_id BLOB NOT NULL, group_name BLOB NOT NULL)"
  rows <- SQL.query_ db "SELECT host_conn_id, member_id, group_name FROM guest_group_self"
  pure [GroupSelf host member name | (host, member, name) <- rows]

groupHostAllowInfo :: B.ByteString
groupHostAllowInfo = "{\"v\":\"1-19\",\"event\":\"x.ok\",\"params\":{}}"

saveHostedGroup :: DBStore -> AgentProtocol.ConnId -> IO ()
saveHostedGroup store connectionId = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_hosted_groups (conn_id BLOB PRIMARY KEY)"
  SQL.execute db "INSERT OR REPLACE INTO guest_hosted_groups (conn_id) VALUES (?)" (SQL.Only connectionId)

hostedGroupConn :: DBStore -> AgentProtocol.ConnId -> IO Bool
hostedGroupConn store connectionId = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_hosted_groups (conn_id BLOB PRIMARY KEY)"
  rows <- SQL.query db "SELECT 1 FROM guest_hosted_groups WHERE conn_id = ? LIMIT 1" (SQL.Only connectionId) :: IO [SQL.Only Int]
  pure (not (null rows))

saveGroupSelf :: DBStore -> IORef [GroupSelf] -> AgentProtocol.ConnId -> B.ByteString -> B.ByteString -> IO ()
saveGroupSelf store selves host member name = do
  withTransaction' store $ \db -> do
    SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_group_self (host_conn_id BLOB PRIMARY KEY, member_id BLOB NOT NULL, group_name BLOB NOT NULL)"
    SQL.execute db "INSERT OR REPLACE INTO guest_group_self (host_conn_id, member_id, group_name) VALUES (?, ?, ?)" (host, member, name)
  atomicModifyIORef' selves $ \known ->
    (GroupSelf host member name : filter ((/= host) . groupSelfHost) known, ())

loadGroupLinks :: DBStore -> IO [GroupLink]
loadGroupLinks store = withTransaction' store $ \db -> do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_group_links (conn_id BLOB PRIMARY KEY, host_conn_id BLOB NOT NULL, member_id BLOB NOT NULL, display_name BLOB NOT NULL)"
  rows <- SQL.query_ db "SELECT conn_id, host_conn_id, member_id, display_name FROM guest_group_links"
  pure [GroupLink conn host member name | (conn, host, member, name) <- rows]

groupSendTargets :: IORef [GroupSelf] -> IORef [GroupLink] -> AgentProtocol.ConnId -> IO [AgentProtocol.ConnId]
groupSendTargets selves links connectionId = do
  knownSelves <- readIORef selves
  knownLinks <- readIORef links
  let host = case find ((== connectionId) . groupSelfHost) knownSelves of
        Just _ -> connectionId
        Nothing -> case find ((== connectionId) . groupLinkConn) knownLinks of
          Just link -> groupLinkHost link
          Nothing -> connectionId
      members = [groupLinkConn link | link <- knownLinks, groupLinkHost link == host, groupLinkConn link /= host]
  pure (host : members)

saveGroupLink :: DBStore -> IORef [GroupLink] -> AgentProtocol.ConnId -> AgentProtocol.ConnId -> B.ByteString -> B.ByteString -> IO ()
saveGroupLink store links conn host member name = do
  withTransaction' store $ \db -> do
    SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_group_links (conn_id BLOB PRIMARY KEY, host_conn_id BLOB NOT NULL, member_id BLOB NOT NULL, display_name BLOB NOT NULL)"
    SQL.execute db "INSERT OR REPLACE INTO guest_group_links (conn_id, host_conn_id, member_id, display_name) VALUES (?, ?, ?, ?)" (conn, host, member, name)
  atomicModifyIORef' links $ \known ->
    (GroupLink conn host member name : filter ((/= conn) . groupLinkConn) known, ())

groupJoinHello :: DBStore -> IORef [GroupInvite] -> B.ByteString -> IO (B.ByteString, Maybe (B.ByteString, B.ByteString))
groupJoinHello store invites uri = do
  known <- readIORef invites
  case find ((== uri) . groupInviteUri) known of
    Just invite -> pure (groupAcceptInfo (groupInviteMember invite), Just (groupInviteMember invite, groupInviteName invite))
    Nothing -> do
      stored <- loadGroupInvites store
      case find ((== uri) . groupInviteUri) stored of
        Just invite -> pure (groupAcceptInfo (groupInviteMember invite), Just (groupInviteMember invite, groupInviteName invite))
        Nothing -> pure ("NexatrodeOS", Nothing)

groupAcceptInfo :: B.ByteString -> B.ByteString
groupAcceptInfo memberId = BL.toStrict . Aeson.encode $ Aeson.object
  [ "v" .= ("1-19" :: String)
  , "event" .= ("x.grp.acpt" :: String)
  , "params" .= Aeson.object ["memberId" .= memberText]
  ]
  where
    memberText = case TE.decodeUtf8' memberId of
      Right text -> text
      Left _ -> Text.empty

groupInviteSuffix :: [(AgentProtocol.ConnId, B.ByteString)] -> IORef [GroupInvite] -> IO B.ByteString
groupInviteSuffix chats invites = do
  known <- readIORef invites
  pure $ case findMatch (zip [0 :: Int ..] chats) known of
    Just (index, uri) | B.length uri <= 2048 ->
      B.singleton (fromIntegral index) <> B.pack (word16LE (fromIntegral (B.length uri))) <> uri
    _ -> B.empty
  where
    findMatch pairs known = find (\(_, (conn, _)) -> any ((== conn) . groupInviteConn) known) pairs
      >>= \(index, (conn, _)) -> (index,) . groupInviteUri <$> find ((== conn) . groupInviteConn) known

sanitizeGroupName :: B.ByteString -> B.ByteString
sanitizeGroupName bytes =
  let cleaned = B.take 32 (B.filter groupNameChar bytes)
  in if B.null cleaned then "Group" else cleaned
  where
    groupNameChar byte = byte == 32 || (byte >= 48 && byte <= 57) || (byte >= 65 && byte <= 90)
      || (byte >= 97 && byte <= 122) || byte == 45 || byte == 46 || byte == 95

displayNameBytes :: B.ByteString -> B.ByteString
displayNameBytes info =
  case findMarker info of
    Just value
      | not (B.null value) && B.length value <= 32 && B.all nameChar value -> value
    _ -> B.empty
  where
    findMarker bytes = go ["\"displayName\":\"", "\"displayName\": \""]
      where
        go [] = Nothing
        go (marker : rest) = case B.breakSubstring marker bytes of
          (_, suffix)
            | B.isPrefixOf marker suffix ->
                Just (B.takeWhile (/= 34) (B.drop (B.length marker) suffix))
            | otherwise -> go rest
    nameChar byte = byte == 32 || (byte >= 48 && byte <= 57) || (byte >= 65 && byte <= 90)
      || (byte >= 97 && byte <= 122) || byte == 45 || byte == 46 || byte == 95

-- Official apps put a small data-URL portrait on the profile. Decode it the
-- same way as an image-message preview so the chat list can show a photo.
profileImageBytes :: B.ByteString -> B.ByteString
profileImageBytes info =
  case findMarker info of
    Just value
      | "data:image/" `Text.isPrefixOf` value -> decodeImagePreview value
    _ -> B.empty
  where
    findMarker bytes = go ["\"image\":\"", "\"image\": \""]
      where
        go [] = Nothing
        go (marker : rest) = case B.breakSubstring marker bytes of
          (_, suffix)
            | B.isPrefixOf marker suffix ->
                let raw = B.takeWhile (/= 34) (B.drop (B.length marker) suffix)
                in if B.null raw
                  then go rest
                  else case TE.decodeUtf8' raw of
                    Right text -> Just text
                    Left _ -> go rest
            | otherwise -> go rest

stableConnectionHandle :: AgentProtocol.ConnId -> Word64
stableConnectionHandle = nonzero . B.foldl' step 14695981039346656037
  where
    step hash byte = (hash `xor` fromIntegral byte) * 1099511628211
    nonzero 0 = 1
    nonzero value = value

word16LE :: Word16 -> [Word8]
word16LE value = [fromIntegral value, fromIntegral (value `shiftR` 8)]

word32LE :: Word32 -> [Word8]
word32LE value = [fromIntegral (value `shiftR` shift) | shift <- [0, 8 .. 24]]

word64LE :: Word64 -> [Word8]
word64LE value = [fromIntegral (value `shiftR` shift) | shift <- [0, 8 .. 56]]
#endif

fixturePin :: B.ByteString -> KeyHash
fixturePin der = case X509.decodeSignedCertificate der of
  Left err -> error ("invalid QEMU SMP fixture CA: " <> err)
  Right cert -> certificateFingerprint cert

awaitAgentEvent :: forall a. IORef [AgentProtocol.ATransmission] -> Agent.AgentClient -> AgentProtocol.ConnId -> (AgentProtocol.ATransmission -> Maybe a) -> ExceptT AgentProtocol.AgentErrorType IO a
awaitAgentEvent backlog client connId matches =
  awaitAgentEventFor 60000000 backlog client connId matches

awaitAgentEventFor
  :: forall a.
     Int
  -> IORef [AgentProtocol.ATransmission]
  -> Agent.AgentClient
  -> AgentProtocol.ConnId
  -> (AgentProtocol.ATransmission -> Maybe a)
  -> ExceptT AgentProtocol.AgentErrorType IO a
awaitAgentEventFor micros backlog client connId matches = do
  result <- liftIO (awaitEventFor micros backlog client connId matches)
  either throwError pure result

awaitEvent :: forall a. IORef [AgentProtocol.ATransmission] -> Agent.AgentClient -> AgentProtocol.ConnId -> (AgentProtocol.ATransmission -> Maybe a) -> IO (Either AgentProtocol.AgentErrorType a)
awaitEvent = awaitEventFor 60000000

awaitEventFor
  :: forall a.
     Int
  -> IORef [AgentProtocol.ATransmission]
  -> Agent.AgentClient
  -> AgentProtocol.ConnId
  -> (AgentProtocol.ATransmission -> Maybe a)
  -> IO (Either AgentProtocol.AgentErrorType a)
awaitEventFor micros backlog client wantedConnId matches = do
  result <- timeout micros loop
  pure $ maybe (Left $ AgentProtocol.INTERNAL "timed out waiting for SimpleX event") id result
  where
    loop = do
      cached <- atomicModifyIORef' backlog $ \events ->
        case break isWantedEvent events of
          (before, []) -> (events, Nothing)
          (before, event : after) -> (before <> after, Just event)
      event <- case cached of
        Just event -> pure event
        Nothing -> atomically $ readTBQueue (Agent.subQ client)
      let (_, eventConnId, _) = event
      case cached of
        Just _ -> pure ()
        Nothing -> putStrLn ("AGENT_EVENT|" <> show eventConnId <> "|" <> eventName event)
      case event of
        (_, connId, AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.ERR err))
          | connId == wantedConnId -> pure (Left err)
        (_, connId, _)
          | connId == wantedConnId -> case matches event of
              Just value -> pure (Right value)
              Nothing -> modifyIORef' backlog (<> [event]) >> loop
        -- Keep empty-entity events (XFTP SFDONE/SFPROG) in the backlog so a
        -- wait for SENT cannot drop them.
        _ -> modifyIORef' backlog (<> [event]) >> loop
    isWantedEvent event
      | transmissionConnId event /= wantedConnId = False
      | otherwise = case event of
          (_, _, AgentProtocol.AEvt AgentProtocol.SAEConn (AgentProtocol.ERR _)) -> True
          _ -> case matches event of
            Just _ -> True
            Nothing -> False

transmissionConnId :: AgentProtocol.ATransmission -> AgentProtocol.ConnId
transmissionConnId (_, connId, _) = connId

eventName :: AgentProtocol.ATransmission -> String
eventName (_, _, AgentProtocol.AEvt _ event) = case event of
  AgentProtocol.CONF {} -> "CONF"
  AgentProtocol.INFO {} -> "INFO"
  AgentProtocol.CON {} -> "CON"
  AgentProtocol.MSG {} -> "MSG"
  AgentProtocol.MSGNTF {} -> "MSGNTF"
  AgentProtocol.SENT {} -> "SENT"
  AgentProtocol.MERR {} -> "MERR"
  AgentProtocol.MERRS {} -> "MERRS"
  AgentProtocol.RCVD {} -> "RCVD"
  AgentProtocol.UP {} -> "UP"
  AgentProtocol.END -> "END"
  AgentProtocol.JOINED {} -> "JOINED"
  AgentProtocol.OK -> "OK"
  AgentProtocol.ERR err -> "ERR|" <> take 240 (show err)
  other -> "OTHER|" <> take 240 (show other)

