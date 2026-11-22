-- | Lifted Directory ops for the guest XFTP ramfs.
module UnliftIO.Directory
  ( doesFileExist,
    createDirectory,
    getFileSize,
    doesPathExist,
    doesDirectoryExist,
    getTemporaryDirectory,
    removeFile,
    removeDirectoryRecursive,
  )
where

import Control.Monad.IO.Class (MonadIO, liftIO)
import qualified System.Directory as Dir

doesFileExist :: MonadIO m => FilePath -> m Bool
doesFileExist = liftIO . Dir.doesFileExist

createDirectory :: MonadIO m => FilePath -> m ()
createDirectory = liftIO . Dir.createDirectory

getFileSize :: MonadIO m => FilePath -> m Integer
getFileSize = liftIO . Dir.getFileSize

doesPathExist :: MonadIO m => FilePath -> m Bool
doesPathExist = liftIO . Dir.doesPathExist

doesDirectoryExist :: MonadIO m => FilePath -> m Bool
doesDirectoryExist = liftIO . Dir.doesDirectoryExist

getTemporaryDirectory :: MonadIO m => m FilePath
getTemporaryDirectory = liftIO Dir.getTemporaryDirectory

removeFile :: MonadIO m => FilePath -> m ()
removeFile = liftIO . Dir.removeFile

removeDirectoryRecursive :: MonadIO m => FilePath -> m ()
removeDirectoryRecursive = liftIO . Dir.removeDirectoryRecursive
