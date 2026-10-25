//! Local-socket transport: a listener for the app and connections for both
//! sides. Unix uses a socket file in the data folder's `control/` subfolder
//! (subfolder 0700, socket 0600; the data folder is left alone); Windows
//! uses a named pipe only the current user can open, rejecting remote clients.
use crate::Endpoint;
use std::{
    fmt,
    io::{self, Read, Write},
    time::Duration,
};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

/// Why the app could not listen.
#[derive(Debug)]
pub enum BindError {
    /// Another running app already answers on this endpoint.
    InUse(String),
    /// Anything else.
    Io(io::Error),
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InUse(endpoint) => write!(
                f,
                "Another MonoCode window already answers on {endpoint}; this one runs without external control."
            ),
            Self::Io(error) => write!(f, "Cannot listen for external control: {error}"),
        }
    }
}

impl std::error::Error for BindError {}

impl From<io::Error> for BindError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// The app's end: accepts connections until dropped. Dropping it removes the
/// socket file on Unix if it is still this listener's.
pub struct Listener {
    inner: imp::Listener,
    endpoint: Endpoint,
}

impl Listener {
    /// Listens on `endpoint`. A stale socket left by a crashed app is
    /// replaced; a live one is refused with [`BindError::InUse`].
    ///
    /// # Errors
    /// [`BindError::InUse`] when another app answers; [`BindError::Io`] otherwise.
    pub fn bind(endpoint: &Endpoint) -> Result<Self, BindError> {
        Ok(Self {
            inner: imp::Listener::bind(endpoint)?,
            endpoint: endpoint.clone(),
        })
    }
    /// Waits for the next connection.
    ///
    /// # Errors
    /// When accepting fails.
    pub fn accept(&self) -> io::Result<Connection> {
        Ok(Connection {
            inner: self.inner.accept()?,
        })
    }
    /// Where this listener listens.
    #[must_use]
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

/// One connection, from either side. `&Connection` reads and writes, so one
/// thread can hold a buffered reader while another writes.
pub struct Connection {
    inner: imp::Stream,
}

impl Connection {
    /// Connects to the app on `endpoint`.
    ///
    /// # Errors
    /// `NotFound` or `ConnectionRefused` when no app listens there.
    pub fn connect(endpoint: &Endpoint) -> io::Result<Self> {
        Ok(Self {
            inner: imp::connect(endpoint)?,
        })
    }
    /// A connected pair, for tests that need no endpoint.
    ///
    /// # Errors
    /// When the platform cannot make one.
    #[cfg(unix)]
    pub fn pair() -> io::Result<(Self, Self)> {
        let (a, b) = std::os::unix::net::UnixStream::pair()?;
        Ok((Self { inner: a }, Self { inner: b }))
    }
    /// Limits how long a read waits. Windows named pipes have no read timeout;
    /// there the app's own reply deadlines bound every wait.
    ///
    /// # Errors
    /// When the platform refuses the timeout.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        #[cfg(unix)]
        return self.inner.set_read_timeout(timeout);
        #[cfg(windows)]
        {
            let _ = timeout;
            Ok(())
        }
    }
    /// Ends the connection both ways, waking a thread blocked reading it. A
    /// no-op on Windows, where such a reader wakes when the app hangs up.
    ///
    /// # Errors
    /// When the platform refuses.
    pub fn shutdown(&self) -> io::Result<()> {
        #[cfg(unix)]
        return self.inner.shutdown(std::net::Shutdown::Both);
        #[cfg(windows)]
        Ok(())
    }
    /// Limits how long a write waits. A no-op on Windows, as for reads.
    ///
    /// # Errors
    /// When the platform refuses the timeout.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        #[cfg(unix)]
        return self.inner.set_write_timeout(timeout);
        #[cfg(windows)]
        {
            let _ = timeout;
            Ok(())
        }
    }
}

impl Read for &Connection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.inner).read(buf)
    }
}

impl Write for &Connection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.inner).write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        (&self.inner).flush()
    }
}

impl Read for Connection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self).read(buf)
    }
}

impl Write for Connection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self).write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        (&*self).flush()
    }
}

/// Whether a connect error means nothing listens there (the app is not
/// running), as opposed to a real failure.
#[must_use]
pub fn nobody_listening(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    ) || imp::nobody_listening(error)
}
