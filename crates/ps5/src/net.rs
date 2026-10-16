//! Blocking IPv4 TCP sockets (BSD sockets in libkernel, no feature needed).
//!
//! ```ignore
//! let server = TcpListener::bind(8090)?;
//! loop {
//!     let mut client = server.accept()?;
//!     client.write_all(b"hello\n")?;
//! }
//! ```

use crate::{Error, Result};
use ps5_sys as sys;

/// `errno` of the last failed call, as an [`Error`].
fn last_error() -> Error {
    Error(unsafe { *sys::__error() })
}

fn cvt(ret: i32) -> Result<i32> {
    if ret < 0 { Err(last_error()) } else { Ok(ret) }
}

/// A listening socket on all interfaces; closed on drop.
pub struct TcpListener {
    fd: i32,
}

impl TcpListener {
    /// Listens on `0.0.0.0:port`.
    pub fn bind(port: u16) -> Result<Self> {
        unsafe {
            let fd = cvt(sys::socket(sys::AF_INET as i32, sys::SOCK_STREAM as i32, 0))?;
            let listener = TcpListener { fd };

            // Lets the payload be restarted right away on the same port.
            let one: i32 = 1;
            cvt(sys::setsockopt(
                fd, sys::SOL_SOCKET as i32, sys::SO_REUSEADDR as i32,
                (&one as *const i32).cast(), size_of::<i32>() as _,
            ))?;

            let mut addr: sys::sockaddr_in = core::mem::zeroed();
            addr.sin_len = size_of::<sys::sockaddr_in>() as u8;
            addr.sin_family = sys::AF_INET as _;
            addr.sin_port = port.to_be();
            addr.sin_addr.s_addr = 0; // INADDR_ANY
            cvt(sys::bind(fd, (&addr as *const sys::sockaddr_in).cast(), size_of::<sys::sockaddr_in>() as _))?;
            cvt(sys::listen(fd, 8))?;
            Ok(listener)
        }
    }

    /// Waits for the next connection. Returns it with the peer's IPv4
    /// address as 4 bytes.
    pub fn accept(&self) -> Result<(TcpStream, [u8; 4])> {
        unsafe {
            let mut addr: sys::sockaddr_in = core::mem::zeroed();
            let mut len = size_of::<sys::sockaddr_in>() as sys::socklen_t;
            let fd = cvt(sys::accept(self.fd, (&mut addr as *mut sys::sockaddr_in).cast(), &mut len))?;
            Ok((TcpStream { fd }, addr.sin_addr.s_addr.to_ne_bytes()))
        }
    }
}

impl Drop for TcpListener {
    fn drop(&mut self) {
        unsafe { sys::close(self.fd) };
    }
}

/// A connected socket; closed on drop.
pub struct TcpStream {
    fd: i32,
}

impl TcpStream {
    /// Reads what's available, blocking until at least one byte arrives;
    /// `Ok(0)` means the peer closed the connection.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let n = unsafe { sys::recv(self.fd, buf.as_mut_ptr().cast(), buf.len(), 0) };
        if n < 0 { Err(last_error()) } else { Ok(n as usize) }
    }

    pub fn write_all(&mut self, mut data: &[u8]) -> Result<()> {
        while !data.is_empty() {
            let n = unsafe { sys::send(self.fd, data.as_ptr().cast(), data.len(), 0) };
            if n < 0 {
                return Err(last_error());
            }
            data = &data[n as usize..];
        }
        Ok(())
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        unsafe { sys::close(self.fd) };
    }
}
