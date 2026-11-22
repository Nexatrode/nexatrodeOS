-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright 2020-2022 simplex.chat
-- NexatrodeOS adaptation (2026-11-19): extract XFTP protocol types for the guest.
{-# LANGUAGE DataKinds #-}
{-# LANGUAGE DeriveAnyClass #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE KindSignatures #-}
{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE PatternSynonyms #-}
{-# LANGUAGE ScopedTypeVariables #-}

module Simplex.FileTransfer.Transport.Types
  ( XFTPErrorType (..), XFTPVersion, blockedFilesXFTPVersion,
    xftpClientHandshakeStub
  ) where

import Control.Monad.Trans.Except (ExceptT, throwE)
import qualified Data.Aeson as J
import qualified Data.Attoparsec.ByteString.Char8 as A
import Data.Word (Word16)
import GHC.Generics (Generic)
import qualified Simplex.Messaging.Crypto as C
import Simplex.Messaging.Encoding (Encoding (..))
import Simplex.Messaging.Encoding.String (StrEncoding (..))
import Simplex.Messaging.Parsers (parseRead1, sumTypeJSON)
import Simplex.Messaging.Protocol (BlockingInfo, CommandError)
import Simplex.Messaging.Transport
  ( ServiceCredentials, THandle, TransportError (..),
    TransportPeer (TClient)
  )
import Simplex.Messaging.Util (bshow)
import Simplex.Messaging.Version (VersionRange, VersionScope)
import Simplex.Messaging.Version.Internal (Version (..))

data XFTPErrorType
  = BLOCK | SESSION | HANDSHAKE | CMD {cmdErr :: CommandError} | AUTH
  | BLOCKED {blockInfo :: BlockingInfo} | SIZE | QUOTA | DIGEST | CRYPTO
  | NO_FILE | HAS_FILE | FILE_IO | TIMEOUT | INTERNAL | DUPLICATE_
  deriving (Eq, Show, Generic)

instance StrEncoding XFTPErrorType where
  strEncode = \case
    BLOCK -> "BLOCK"
    SESSION -> "SESSION"
    HANDSHAKE -> "HANDSHAKE"
    CMD e -> "CMD " <> bshow e
    AUTH -> "AUTH"
    BLOCKED info -> "BLOCKED " <> strEncode info
    SIZE -> "SIZE"
    QUOTA -> "QUOTA"
    DIGEST -> "DIGEST"
    CRYPTO -> "CRYPTO"
    NO_FILE -> "NO_FILE"
    HAS_FILE -> "HAS_FILE"
    FILE_IO -> "FILE_IO"
    TIMEOUT -> "TIMEOUT"
    INTERNAL -> "INTERNAL"
    DUPLICATE_ -> "DUPLICATE_"
  strP = A.takeTill (== ' ') >>= \case
    "BLOCK" -> pure BLOCK
    "SESSION" -> pure SESSION
    "HANDSHAKE" -> pure HANDSHAKE
    "CMD" -> CMD <$> parseRead1
    "AUTH" -> pure AUTH
    "BLOCKED" -> BLOCKED <$> strP
    "SIZE" -> pure SIZE
    "QUOTA" -> pure QUOTA
    "DIGEST" -> pure DIGEST
    "CRYPTO" -> pure CRYPTO
    "NO_FILE" -> pure NO_FILE
    "HAS_FILE" -> pure HAS_FILE
    "FILE_IO" -> pure FILE_IO
    "TIMEOUT" -> pure TIMEOUT
    "INTERNAL" -> pure INTERNAL
    "DUPLICATE_" -> pure DUPLICATE_
    _ -> fail "bad error type"

instance Encoding XFTPErrorType where
  smpEncode = strEncode
  smpP = strP

instance J.ToJSON XFTPErrorType where
  toJSON = J.genericToJSON (sumTypeJSON id)
  toEncoding = J.genericToEncoding (sumTypeJSON id)

instance J.FromJSON XFTPErrorType where
  parseJSON = J.genericParseJSON (sumTypeJSON id)

data XFTPVersion
instance VersionScope XFTPVersion
type VersionXFTP = Version XFTPVersion
type VersionRangeXFTP = VersionRange XFTPVersion
pattern VersionXFTP :: Word16 -> VersionXFTP
pattern VersionXFTP v = Version v

blockedFilesXFTPVersion :: VersionXFTP
blockedFilesXFTPVersion = VersionXFTP 3

xftpClientHandshakeStub :: c 'TClient -> Maybe C.KeyPairX25519 -> C.KeyHash -> VersionRangeXFTP -> Bool -> Maybe (ServiceCredentials, C.KeyPairEd25519) -> ExceptT TransportError IO (THandle XFTPVersion c 'TClient)
xftpClientHandshakeStub _ _ _ _ _ _ = throwE TEVersion
