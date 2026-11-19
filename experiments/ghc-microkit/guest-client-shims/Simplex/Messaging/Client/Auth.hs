{-# LANGUAGE DataKinds #-}
{-# LANGUAGE GADTs #-}
{-# LANGUAGE ScopedTypeVariables #-}
{-# LANGUAGE TupleSections #-}
{-# LANGUAGE TypeApplications #-}

module Simplex.Messaging.Client.Auth (authTransmission) where

import qualified Data.X509.Validation as XV
import Data.ByteString.Char8 (ByteString)
import qualified Simplex.Messaging.Crypto as C
import Simplex.Messaging.Protocol (TAuthorizations, TransmissionAuth (..))
import Simplex.Messaging.Transport
  ( THClientService' (..), THandleAuth (..), TransportError (..),
    TransportPeer (TClient)
  )

-- Kept separate from the networking client so protocol-only modules can use
-- the exact upstream signing/authenticator operation without importing sockets.
authTransmission :: Maybe (THandleAuth 'TClient) -> Bool -> Maybe C.APrivateAuthKey -> C.CbNonce -> ByteString -> Either TransportError (Maybe TAuthorizations)
authTransmission thAuth serviceAuth pKey_ nonce t = traverse authenticate pKey_
  where
    authenticate :: C.APrivateAuthKey -> Either TransportError TAuthorizations
    authenticate (C.APrivateAuthKey a pk) = (,serviceSig) <$> case a of
      C.SX25519 -> case thAuth of
        Just THAuthClient {peerServerPubKey = k} -> Right $ TAAuthenticator $ C.cbAuthenticate k pk nonce t'
        Nothing -> Left TENoServerAuth
      C.SEd25519 -> sign pk
      C.SEd448 -> sign pk
    (t', serviceSig) = case clientService =<< thAuth of
      Just THClientService {serviceCertHash = XV.Fingerprint fp, serviceKey} | serviceAuth ->
        (fp <> t, Just $ C.sign' serviceKey t)
      _ -> (t, Nothing)
    sign :: forall a. (C.AlgorithmI a, C.SignatureAlgorithm a) => C.PrivateKey a -> Either TransportError TransmissionAuth
    sign pk = Right $ TASignature $ C.ASignature (C.sAlgorithm @a) (C.sign' pk t')
