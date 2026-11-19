module SimplexUploads (PendingXftpSend (..), takePendingUpload) where

import qualified Data.ByteString as B
import Data.IORef (IORef, atomicModifyIORef')
import Data.Int (Int64)

data PendingXftpSend = PendingXftpSend
  { pendingXftpSndId :: B.ByteString
  , pendingXftpConn :: B.ByteString
  , pendingXftpSharedMsg :: B.ByteString
  , pendingOfferId :: Int64
  } deriving (Eq, Show)

-- Success and terminal errors both release exactly the transfer named by XFTP.
takePendingUpload :: IORef [PendingXftpSend] -> B.ByteString -> IO (Maybe PendingXftpSend)
takePendingUpload pending fileId = atomicModifyIORef' pending $ \uploads ->
  case break ((== fileId) . pendingXftpSndId) uploads of
    (before, found : after) -> (before ++ after, Just found)
    (_, []) -> (uploads, Nothing)
