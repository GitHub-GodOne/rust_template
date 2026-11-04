//! TCP listeners that child processes cannot inherit.
//!
//! On Windows tokio (mio) creates sockets with plain `socket()`, which makes
//! them inheritable, and every process the server starts (项目命令 runs,
//! terminals, gateways) is spawned with handle inheritance on. A long-running
//! child then holds a copy of the listening socket: after the server exits the
//! port stays taken and accepts connections nobody answers. socket2 creates
//! the socket with `WSA_FLAG_NO_HANDLE_INHERIT` (`SOCK_CLOEXEC` elsewhere), and
//! accepted connections go through std, which marks them non-inheritable too.

use std::{io, net::SocketAddr};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpListener;

/// Same backlog as `tokio::net::TcpListener::bind`.
const BACKLOG: i32 = 1024;

/// Like `tokio::net::TcpListener::bind`, but the socket is never inherited.
///
/// # Errors
///
/// When the address does not resolve or no resolved address can be bound.
pub async fn bind(address: &str) -> io::Result<TcpListener> {
    let mut last_error = None;
    for addr in tokio::net::lookup_host(address).await? {
        match listen(addr) {
            Ok(listener) => return Ok(listener),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{address} did not resolve to any address"),
        )
    }))
}

fn listen(addr: SocketAddr) -> io::Result<TcpListener> {
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    // As tokio does: lets a restarted server rebind a port in TIME_WAIT. On
    // Windows the flag would instead let other sockets take over the port.
    #[cfg(not(windows))]
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    socket.listen(BACKLOG)?;
    TcpListener::from_std(socket.into())
}

#[cfg(test)]
mod tests {
    use super::bind;

    #[tokio::test]
    async fn binds_and_accepts() {
        let listener = bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (connected, accepted) =
            tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
        connected.unwrap();
        accepted.unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn listener_and_connections_are_not_inheritable() {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Foundation::{GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};

        fn inheritable(socket: &impl AsRawSocket) -> bool {
            let mut flags = 0;
            // SAFETY: the socket is open for the duration of the call.
            let queried =
                unsafe { GetHandleInformation(socket.as_raw_socket() as HANDLE, &raw mut flags) };
            assert_ne!(queried, 0);
            flags & HANDLE_FLAG_INHERIT != 0
        }

        let listener = bind("127.0.0.1:0").await.unwrap();
        assert!(!inheritable(&listener));
        let address = listener.local_addr().unwrap();
        let (_client, accepted) =
            tokio::join!(tokio::net::TcpStream::connect(address), listener.accept());
        assert!(!inheritable(&accepted.unwrap().0));
    }
}
