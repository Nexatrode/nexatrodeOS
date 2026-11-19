-- The HTTP/2 client supplies confReadN callbacks. The upstream socket-based
-- readN helper is server/host convenience code and is not part of the guest.
module Network.HTTP2.Arch.ReadN where
