-- SPDX-License-Identifier: AGPL-3.0-only
-- Copyright 2020-2022 simplex.chat
-- NexatrodeOS extraction for the guest (2026-11-19).
-- | Guest-only narrow dependency of upstream Encoding.hs. This definition is
-- identical to Simplex.Messaging.Util.(<$?>) in SimpleXMQ 7.0.0.6.
module Simplex.Messaging.Util ((<$?>)) where

(<$?>) :: MonadFail m => (a -> Either String b) -> m a -> m b
f <$?> m = either fail pure . f =<< m
