{-# LANGUAGE ForeignFunctionInterface #-}

module Main (main) where

import Control.Concurrent (forkFinally, killThread, newEmptyMVar, putMVar, takeMVar, threadDelay)
import Control.Exception (throwIO)
import Control.Monad (unless)
import Foreign.C.Types (CInt(..))
import NativeNetwork

foreign import ccall unsafe "test_tcp_reset" resetTCP :: IO ()
foreign import ccall unsafe "test_tcp_allow_connect" allowConnect :: CInt -> IO ()
foreign import ccall unsafe "test_tcp_state" tcpState :: IO CInt

assert :: Bool -> String -> IO ()
assert condition message = unless condition (throwIO (userError message))

main :: IO ()
main = do
  resetTCP
  first <- connectIPv4 (IPv4 127 0 0 1) 5223 50
  assert (first == Left TimedOut) "pending connect should time out"
  state <- tcpState
  assert (state == 0) "timed-out connection left its TCP slot occupied"
  allowConnect 1
  second <- connectIPv4 (IPv4 127 0 0 1) 5223 50
  assert (second == Right ()) "slot could not be reused after timeout"
  cancelSession

  resetTCP
  allowConnect (-1)
  failed <- connectIPv4 (IPv4 127 0 0 1) 5223 50
  assert (failed == Left TransportError) "connection failure was not reported"
  failedState <- tcpState
  assert (failedState == 0) "failed connection left its TCP slot occupied"

  resetTCP
  finished <- newEmptyMVar
  worker <- forkFinally (connectIPv4 (IPv4 127 0 0 1) 5223 5000) (putMVar finished)
  let waitForStart 0 = throwIO (userError "connection attempt did not start")
      waitForStart remaining = do
        state' <- tcpState
        unless (state' == 1) $ threadDelay 10000 >> waitForStart (remaining - 1)
  waitForStart (100 :: Int)
  killThread worker
  _ <- takeMVar finished
  cancelledState <- tcpState
  assert (cancelledState == 0) "cancelled connect left its TCP slot occupied"
  allowConnect 1
  third <- connectIPv4 (IPv4 127 0 0 1) 5223 50
  assert (third == Right ()) "slot could not be reused after cancellation"
  busy <- connectIPv4 (IPv4 127 0 0 1) 5223 50
  assert (busy == Left Busy) "an occupied connection should report busy"
  stillConnected <- tcpState
  assert (stillConnected == 2) "a busy attempt cancelled the existing connection"
  oldEpoch <- currentEpoch 0
  cancelSession
  fourth <- connectIPv4 (IPv4 127 0 0 1) 5223 50
  assert (fourth == Right ()) "slot could not be reopened"
  cancelSessionEpoch 0 oldEpoch
  replacementState <- tcpState
  assert (replacementState == 2) "a stale owner cancelled the replacement"
  cancelSession
  putStrLn "Native network connection recovery: PASS"
