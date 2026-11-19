-- Guest subset of Data.Yaml for the SimpleX file-description boundary.
-- Official clients encode fileDescr as a constrained YAML dialect (see
-- Simplex.FileTransfer.Description). Emit that dialect on send so Phone can
-- parseFileDescription; decode accepts JSON first, then YAML — no libyaml.
{-# LANGUAGE OverloadedStrings #-}

module Data.Yaml
  ( encode,
    decodeEither',
    parseJSON,
    toJSON,
  )
where

import Data.Aeson (FromJSON, ToJSON, Value (..), parseJSON, toJSON)
import qualified Data.Aeson as Aeson
import qualified Data.Aeson.Key as Key
import qualified Data.Aeson.KeyMap as KeyMap
import Data.ByteString (ByteString)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as BC
import Data.Char (isSpace, ord)
import Data.Text (Text)
import Data.Word (Word8)
import qualified Data.Text.Encoding as TE
import qualified Data.Text.Encoding.Error as TEE
import Data.Vector (fromList, toList)

encode :: ToJSON a => a -> ByteString
encode = encodeValue . toJSON

-- | Official fileDescr text is YAML, not JSON. Phone's parseFileDescription
-- uses real Data.Yaml and ignores our older JSON strEncode output.
encodeValue :: Value -> ByteString
encodeValue v = BC.intercalate "\n" (encodeNode 0 v) <> "\n"

encodeNode :: Int -> Value -> [ByteString]
encodeNode indent value = case value of
  Object km ->
    concatMap (encodePair indent) (KeyMap.toList km)
  Array xs ->
    concatMap (encodeListItem indent) (toList xs)
  String t -> [pad indent <> encodeScalar t]
  Number n -> [pad indent <> BC.pack (show n)]
  Bool True -> [pad indent <> "true"]
  Bool False -> [pad indent <> "false"]
  Null -> [pad indent <> ""]

encodePair :: Int -> (Key.Key, Value) -> [ByteString]
encodePair indent (key, value) =
  let k = TE.encodeUtf8 (Key.toText key)
   in case value of
        Object km
          | KeyMap.null km -> [pad indent <> k <> ":"]
          | otherwise -> (pad indent <> k <> ":") : concatMap (encodePair (indent + 2)) (KeyMap.toList km)
        Array xs
          | null (toList xs) -> [pad indent <> k <> ":"]
          -- Official replicas keep the dash at the same indent as the key.
          | otherwise -> (pad indent <> k <> ":") : concatMap (encodeListItem indent) (toList xs)
        String t -> [pad indent <> k <> ": " <> encodeScalar t]
        Number n -> [pad indent <> k <> ": " <> BC.pack (show n)]
        Bool True -> [pad indent <> k <> ": true"]
        Bool False -> [pad indent <> k <> ": false"]
        Null -> [pad indent <> k <> ":"]

encodeListItem :: Int -> Value -> [ByteString]
encodeListItem indent value = case value of
  Object km ->
    case KeyMap.toList km of
      [] -> [pad indent <> "-"]
      ((key, v) : rest) ->
        let k = TE.encodeUtf8 (Key.toText key)
            headLine = case v of
              Object {} -> pad indent <> "- " <> k <> ":"
              Array xs
                | null (toList xs) -> pad indent <> "- " <> k <> ":"
                | otherwise -> pad indent <> "- " <> k <> ":"
              String t -> pad indent <> "- " <> k <> ": " <> encodeScalar t
              Number n -> pad indent <> "- " <> k <> ": " <> BC.pack (show n)
              Bool True -> pad indent <> "- " <> k <> ": true"
              Bool False -> pad indent <> "- " <> k <> ": false"
              Null -> pad indent <> "- " <> k <> ":"
            nested = case v of
              Object km' -> concatMap (encodePair (indent + 2)) (KeyMap.toList km')
              Array xs -> concatMap (encodeListItem (indent + 2)) (toList xs)
              _ -> []
            siblings = concatMap (encodePair (indent + 2)) rest
         in headLine : nested ++ siblings
  Array xs ->
    (pad indent <> "-") : concatMap (encodeListItem (indent + 2)) (toList xs)
  String t -> [pad indent <> "- " <> encodeScalar t]
  Number n -> [pad indent <> "- " <> BC.pack (show n)]
  Bool True -> [pad indent <> "- true"]
  Bool False -> [pad indent <> "- false"]
  Null -> [pad indent <> "-"]

encodeScalar :: Text -> ByteString
encodeScalar = TE.encodeUtf8

pad :: Int -> ByteString
pad n = B.replicate n 32

decodeEither' :: FromJSON a => ByteString -> Either String a
decodeEither' bs = case Aeson.eitherDecodeStrict' bs of
  Right a -> Right a
  Left jsonErr -> case yamlDialectToValue bs of
    Left yamlErr ->
      Left $ "JSON: " <> jsonErr <> "; YAML: " <> yamlErr
    Right val -> case Aeson.fromJSON val of
      Aeson.Error e -> Left e
      Aeson.Success a -> Right a

-- | Constrained YAML → JSON for SimpleX FileDescription documents:
-- maps of scalars, nested maps, and lists of maps/scalars. No anchors,
-- tags, or flow collections — matching official file_description.yaml.
yamlDialectToValue :: ByteString -> Either String Value
yamlDialectToValue bs = do
  let nonEmpty = filter (not . B.null . trim) (map stripKeepHash (BC.lines bs))
  case nonEmpty of
    [] -> Left "empty YAML"
    _ -> do
      (node, rest) <- parseBlock 0 nonEmpty
      if null rest then Right (nodeToValue node) else Left "trailing YAML content"

data YNode
  = YMap [(ByteString, YNode)]
  | YList [YNode]
  | YStr ByteString
  deriving (Show)

nodeToValue :: YNode -> Value
nodeToValue (YStr s) = String (decodeUtf8Lenient s)
nodeToValue (YList xs) = Array (fromList (map nodeToValue xs))
nodeToValue (YMap kvs) =
  Object $ KeyMap.fromList [(Key.fromText (decodeUtf8Lenient k), nodeToValue v) | (k, v) <- kvs]

decodeUtf8Lenient :: ByteString -> Text
decodeUtf8Lenient = TE.decodeUtf8With TEE.lenientDecode

-- Keep `#` inside values (xftp URIs); only strip full-line comments.
stripKeepHash :: ByteString -> ByteString
stripKeepHash line =
  let t = trim line
   in if B.null t || BC.head t == '#' then B.empty else line

indentOf :: ByteString -> Int
indentOf = B.length . B.takeWhile (\c -> c == 32 || c == 9)

trim :: ByteString -> ByteString
trim = B.reverse . B.dropWhile isSp . B.reverse . B.dropWhile isSp
  where
    isSp c = c == 32 || c == 9 || c == 13

c8 :: Char -> Word8
c8 = fromIntegral . ord

isSpaceW8 :: Word8 -> Bool
isSpaceW8 w = w == 32 || w == 9 || w == 13

isListItem :: ByteString -> Bool
isListItem line =
  let t = trim line
   in B.length t >= 2 && BC.head t == '-' && isSpace (BC.index t 1)
      || (B.length t == 1 && BC.head t == '-')

nextContentIndent :: [ByteString] -> Int -> Int
nextContentIndent [] base = base
nextContentIndent (l : ls) base
  | B.null (trim l) = nextContentIndent ls base
  | otherwise = indentOf l

splitKeyValue :: ByteString -> Maybe (ByteString, ByteString)
splitKeyValue line =
  case BC.elemIndex ':' line of
    Nothing -> Nothing
    Just i ->
      let key = trim (B.take i line)
          rest = B.drop (i + 1) line
       in if B.null key then Nothing else Just (key, rest)

parseBlock :: Int -> [ByteString] -> Either String (YNode, [ByteString])
parseBlock _ [] = Left "unexpected end of YAML"
parseBlock baseIndent lines@(l : _)
  | isListItem l = parseList baseIndent lines
  | otherwise = do
      (pairs, rest) <- parseMapEntries baseIndent lines
      Right (YMap pairs, rest)

parseMapEntries :: Int -> [ByteString] -> Either String ([(ByteString, YNode)], [ByteString])
parseMapEntries baseIndent = go []
  where
    go acc [] = Right (reverse acc, [])
    go acc (l : ls)
      | B.null (trim l) = go acc ls
      | indentOf l < baseIndent = Right (reverse acc, l : ls)
      | indentOf l > baseIndent = Left "unexpected deeper indent in map"
      | isListItem l = Right (reverse acc, l : ls)
      | otherwise = case splitKeyValue (trim l) of
          Nothing -> Left $ "expected key: value, got: " <> BC.unpack (B.take 80 l)
          Just (key, valRest)
            | not (B.null (trim valRest)) ->
                go ((key, YStr (trim valRest)) : acc) ls
            | otherwise -> case ls of
                -- Official file descriptions use:
                --   replicas:
                --   - chunks: …
                -- with the sequence dash at the same indent as the key.
                (n : _)
                  | not (B.null (trim n))
                  , isListItem n
                  , indentOf n == baseIndent -> do
                      (child, rest) <- parseList baseIndent ls
                      go ((key, child) : acc) rest
                _ -> do
                  let childIndent = nextContentIndent ls baseIndent
                  if childIndent <= baseIndent
                    then go ((key, YStr B.empty) : acc) ls
                    else do
                      (child, rest) <- parseBlock childIndent ls
                      go ((key, child) : acc) rest

parseList :: Int -> [ByteString] -> Either String (YNode, [ByteString])
parseList baseIndent = go []
  where
    go acc [] = Right (YList (reverse acc), [])
    go acc (l : ls)
      | B.null (trim l) = go acc ls
      | indentOf l < baseIndent = Right (YList (reverse acc), l : ls)
      | indentOf l > baseIndent = Left "unexpected deeper indent in list"
      | not (isListItem l) = Right (YList (reverse acc), l : ls)
      | otherwise = do
          let afterDash = trim (B.dropWhile (== c8 '-') (trim l))
              afterDash' =
                if B.null afterDash
                  then afterDash
                  else trim (B.dropWhile isSpaceW8 afterDash)
          -- `-` alone, or `- key: …`
          case afterDash' of
            s
              | B.null s -> do
                  let childIndent = nextContentIndent ls baseIndent
                  if childIndent <= baseIndent
                    then go (YStr B.empty : acc) ls
                    else do
                      (child, rest) <- parseBlock childIndent ls
                      go (child : acc) rest
              -- Chunk replica strings look like `1:digest:key:…` — scalars with
              -- colons. Only treat `- name:` as a map when the key starts with a letter.
              | Just (key, valRest) <- splitKeyValue s
              , not (B.null key)
              , let c = BC.head key
              , (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') -> do
                  (item, rest) <- parseDashedMapItem key valRest ls baseIndent
                  go (item : acc) rest
              | otherwise -> go (YStr s : acc) ls

-- Official replica items look like:
--   - chunks:
--     - 1:digest:…
--     server: xftp://…
-- or inline `- server: …` with optional deeper sibling keys.
parseDashedMapItem ::
  ByteString ->
  ByteString ->
  [ByteString] ->
  Int ->
  Either String (YNode, [ByteString])
parseDashedMapItem key valRest ls baseIndent
  | not (B.null (trim valRest)) = do
      let childIndent = nextContentIndent ls baseIndent
      if childIndent > baseIndent
        then do
          (more, rest) <- parseMapEntries childIndent ls
          Right (YMap ((key, YStr (trim valRest)) : more), rest)
        else Right (YMap [(key, YStr (trim valRest))], ls)
  | otherwise = do
      let childIndent = nextContentIndent ls baseIndent
      if childIndent <= baseIndent
        then Right (YMap [(key, YStr B.empty)], ls)
        else do
          (child, afterChild) <- parseBlock childIndent ls
          -- Sibling map keys at the same indent as `key` under this list item
          -- (e.g. `server:` after nested `chunks:` list).
          (siblings, rest) <- parseMapEntries childIndent afterChild
          Right (YMap ((key, child) : siblings), rest)
