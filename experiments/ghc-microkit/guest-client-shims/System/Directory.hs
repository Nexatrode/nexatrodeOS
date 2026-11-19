-- | Guest filesystem for the XFTP work tree. Paths are backed by the
-- sel4_libc ramfs (open/mkdir/unlink), not a host mount.
module System.Directory
  ( getFileSize,
    createDirectory,
    createDirectoryIfMissing,
    doesFileExist,
    doesDirectoryExist,
    doesPathExist,
    getTemporaryDirectory,
    removeFile,
    removeDirectoryRecursive,
  )
where

import Control.Exception (ioError)
import Foreign.C.Error (Errno (..), eEXIST, eNOENT, getErrno)
import Foreign.C.String (CString, withCString)
import Foreign.C.Types (CInt (..), CLong (..), CUInt (..))
import System.IO.Error (userError)

foreign import ccall unsafe "mkdir" c_mkdir :: CString -> CUInt -> IO CInt
foreign import ccall unsafe "access" c_access :: CString -> CInt -> IO CInt
foreign import ccall unsafe "unlink" c_unlink :: CString -> IO CInt
foreign import ccall unsafe "sel4_path_size" c_path_size :: CString -> IO CLong
foreign import ccall unsafe "sel4_path_is_dir" c_path_is_dir :: CString -> IO CInt

failErrno :: String -> IO a
failErrno label = do
  Errno code <- getErrno
  ioError (userError (label <> ": errno " <> show code))

createDirectory :: FilePath -> IO ()
createDirectory path = withCString path $ \cPath -> do
  result <- c_mkdir cPath 0o700
  if result == 0
    then pure ()
    else do
      err <- getErrno
      if err == eEXIST then pure () else failErrno "createDirectory"

createDirectoryIfMissing :: Bool -> FilePath -> IO ()
createDirectoryIfMissing _ = createDirectory

doesPathExist :: FilePath -> IO Bool
doesPathExist path = withCString path $ \cPath -> do
  result <- c_access cPath 0
  pure (result == 0)

doesFileExist :: FilePath -> IO Bool
doesFileExist path = withCString path $ \cPath -> do
  exists <- c_access cPath 0
  if exists /= 0
    then pure False
    else do
      isDir <- c_path_is_dir cPath
      pure (isDir == 0)

doesDirectoryExist :: FilePath -> IO Bool
doesDirectoryExist path = withCString path $ \cPath -> do
  exists <- c_access cPath 0
  if exists /= 0
    then pure False
    else do
      isDir <- c_path_is_dir cPath
      pure (isDir == 1)

getFileSize :: FilePath -> IO Integer
getFileSize path = withCString path $ \cPath -> do
  size <- c_path_size cPath
  if size < 0 then failErrno "getFileSize" else pure (fromIntegral size)

getTemporaryDirectory :: IO FilePath
getTemporaryDirectory = pure "xftp"

removeFile :: FilePath -> IO ()
removeFile path = withCString path $ \cPath -> do
  result <- c_unlink cPath
  if result == 0
    then pure ()
    else do
      err <- getErrno
      if err == eNOENT then pure () else failErrno "removeFile"

removeDirectoryRecursive :: FilePath -> IO ()
removeDirectoryRecursive _ = pure ()
