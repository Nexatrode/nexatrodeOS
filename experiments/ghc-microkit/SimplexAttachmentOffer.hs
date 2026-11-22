{-# LANGUAGE OverloadedStrings #-}
module SimplexAttachmentOffer where

import Control.Monad (forM_)
import Data.Aeson ((.:?), (.!=), withObject)
import qualified Data.Aeson as Aeson
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import Data.Int (Int64)
import qualified Data.Text as T
import qualified Data.Text.Encoding as TE
import qualified Database.SQLite.Simple as SQL

data AttachmentOffer = AttachmentOffer
  { offerSharedId :: Maybe T.Text
  , offerName :: T.Text
  , offerDescription :: Maybe (T.Text, Bool)
  } deriving (Eq, Show)

parseAttachmentOffer :: Maybe T.Text -> Aeson.Object -> Parser AttachmentOffer
parseAttachmentOffer shared params = do
  offered <- params .:? "file"
  case offered of
    Nothing -> pure (AttachmentOffer shared "file" Nothing)
    Just value -> flip (withObject "SimpleX file") value $ \file -> do
      name <- file .:? "fileName" .!= "file"
      descr <- file .:? "fileDescr" >>= traverse (withObject "SimpleX file description" $ \fields ->
        (,) <$> (fields .:? "fileDescrText" .!= "") <*> (fields .:? "fileDescrComplete" .!= False))
      pure (AttachmentOffer shared (T.take 80 name) descr)

setupAttachmentOffers :: SQL.Connection -> IO ()
setupAttachmentOffers db = do
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_attachment_offers (conn_id BLOB NOT NULL, shared_id BLOB NOT NULL, name BLOB NOT NULL, msg_id INTEGER NOT NULL, part INTEGER NOT NULL, PRIMARY KEY (conn_id, shared_id))"
  -- An optional caption identifies image offers, including those with an empty caption.
  SQL.execute_ db "CREATE TABLE IF NOT EXISTS guest_attachment_receipts (conn_id BLOB NOT NULL, msg_id INTEGER NOT NULL, part INTEGER NOT NULL, caption BLOB, complete INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (conn_id, msg_id, part))"

rememberAttachmentOffer :: SQL.Connection -> B.ByteString -> AttachmentOffer -> Int64 -> Int64 -> Maybe B.ByteString -> IO (Int64, Int64)
rememberAttachmentOffer db conn offer msg part caption = do
  (row, rowPart) <- case offerSharedId offer of
    Nothing -> pure (msg, part)
    Just shared -> do
      SQL.execute db "INSERT OR IGNORE INTO guest_attachment_offers (conn_id, shared_id, name, msg_id, part) VALUES (?, ?, ?, ?, ?)"
        (conn, TE.encodeUtf8 shared, TE.encodeUtf8 (offerName offer), msg, part)
      stored <- lookupAttachmentOffer db conn shared
      pure $ maybe (msg, part) (\(_, m, p) -> (m, p)) stored
  forM_ caption $ \text ->
    SQL.execute db "INSERT OR IGNORE INTO guest_attachment_receipts (conn_id, msg_id, part, caption) VALUES (?, ?, ?, ?)"
      (conn, row, rowPart, text)
  pure (row, rowPart)

lookupAttachmentOffer :: SQL.Connection -> B.ByteString -> T.Text -> IO (Maybe (B.ByteString, Int64, Int64))
lookupAttachmentOffer db conn shared = do
  rows <- SQL.query db "SELECT name, msg_id, part FROM guest_attachment_offers WHERE conn_id = ? AND shared_id = ?"
    (conn, TE.encodeUtf8 shared)
  pure $ case rows of
    row : _ -> Just row
    [] -> Nothing

attachmentReceipt :: SQL.Connection -> B.ByteString -> Int64 -> Int64 -> IO (Maybe B.ByteString, Bool)
attachmentReceipt db conn msg part = do
  rows <- SQL.query db "SELECT caption, complete FROM guest_attachment_receipts WHERE conn_id = ? AND msg_id = ? AND part = ?"
    (conn, msg, part)
  pure $ case rows of
    row : _ -> row
    [] -> (Nothing, False)

markAttachmentComplete :: SQL.Connection -> B.ByteString -> Int64 -> Int64 -> IO ()
markAttachmentComplete db conn msg part =
  SQL.execute db "INSERT INTO guest_attachment_receipts (conn_id, msg_id, part, complete) VALUES (?, ?, ?, 1) ON CONFLICT (conn_id, msg_id, part) DO UPDATE SET complete = 1"
    (conn, msg, part)
