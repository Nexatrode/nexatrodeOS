-- HTTP/2 peer-address metadata only. Network connections are supplied by the
-- Microkit network service, never by this compatibility type.
module Network.Socket (SockAddr (..), HostName, ServiceName, PortNumber) where

type HostName = String

type ServiceName = String

-- Numeric placeholder needed only by the disabled remote-control listener API.
type PortNumber = Int

data SockAddr = SockAddrInet Int Int
  deriving (Eq, Show)
