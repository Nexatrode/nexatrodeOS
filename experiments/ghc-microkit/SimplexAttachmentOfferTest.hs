{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Exception (bracket)
import Control.Monad (unless)
import qualified Data.Aeson as Aeson
import Data.Aeson.Types (parseEither)
import qualified Database.SQLite.Simple as SQL
import SimplexAttachmentOffer
import System.Directory (getTemporaryDirectory, removeFile)
import System.IO (openTempFile, hClose)

main :: IO ()
main = do
  let event = "{\"msgId\":\"shared\",\"event\":\"x.msg.new\",\"params\":{\"content\":{\"type\":\"image\",\"text\":\"Cliffs\"},\"file\":{\"fileName\":\"coast.png\",\"fileDescr\":{\"fileDescrText\":\"descriptor\",\"fileDescrComplete\":true}}}}"
  value <- either fail pure (Aeson.eitherDecodeStrict' event)
  offer <- either fail pure $ parseEither (Aeson.withObject "event" $ \o -> do
    shared <- o Aeson..:? "msgId"
    params <- o Aeson..: "params"
    parseAttachmentOffer shared params) value
  unless (offer == AttachmentOffer (Just "shared") "coast.png" (Just ("descriptor", True))) $
    fail "image offer lost its shared ID or inline description"
  tmp <- getTemporaryDirectory
  bracket (do (p,h) <- openTempFile tmp "simplex-offer.db"; hClose h; pure p) removeFile $ \path -> do
    let open = bracket (SQL.open path) SQL.close
    open $ \db -> do
      -- A vault created by the previous guest already has this table.
      SQL.execute_ db "CREATE TABLE guest_attachment_offers (conn_id BLOB NOT NULL, shared_id BLOB NOT NULL, name BLOB NOT NULL, msg_id INTEGER NOT NULL, part INTEGER NOT NULL, PRIMARY KEY (conn_id, shared_id))"
      setupAttachmentOffers db
      _ <- rememberAttachmentOffer db "alice" offer 73 2 (Just "Cliffs")
      _ <- rememberAttachmentOffer db "bob" offer 99 0 Nothing
      pure ()
    open $ \db -> do
      setupAttachmentOffers db
      row <- lookupAttachmentOffer db "alice" "shared"
      other <- lookupAttachmentOffer db "bob" "shared"
      unless (row == Just ("coast.png", 73, 2) && other == Just ("coast.png", 99, 0)) $
        fail "delayed description did not find its own chat, message and batch part"
      receipt <- attachmentReceipt db "alice" 73 2
      unless (receipt == (Just "Cliffs", False)) $ fail "lost image caption on reopen"
      markAttachmentComplete db "alice" 73 2
    open $ \db -> do
      target <- rememberAttachmentOffer db "alice" offer 88 3 (Just "replayed caption")
      unless (target == (73, 2)) $ fail "a replay with a new envelope created another photo row"
      receipt <- attachmentReceipt db "alice" 73 2
      other <- attachmentReceipt db "bob" 99 0
      unless (receipt == (Just "Cliffs", True) && other == (Nothing, False)) $
        fail "replayed offer or reopened database reset the completed photo"
  putStrLn "SimpleX image offer and XFTP correlation: PASS"
