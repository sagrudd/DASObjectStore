//! One connection, one inherited monotonic deadline; no retry or renewed budget.
use dasobjectstore_object_service::custody::CustodyReadDeadline;
use dasobjectstore_object_service::custody_reader::{wire::http, ReaderError};
use rustls::Connection;
use std::io::{self, Read, Write};
use std::net::TcpStream;

struct DeadlineSocket {
    stream: TcpStream,
    deadline: CustodyReadDeadline,
}
impl DeadlineSocket {
    fn remaining(&self) -> io::Result<std::time::Duration> {
        self.deadline
            .remaining()
            .map_err(|_| io::ErrorKind::TimedOut.into())
    }
}
impl Read for DeadlineSocket {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(out)
    }
}
impl Write for DeadlineSocket {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.remaining().map(|_| ())
    }
}
pub(super) struct Channel {
    connection: Connection,
    socket: DeadlineSocket,
}
impl Channel {
    pub(super) fn authenticate(
        mut connection: Connection,
        stream: TcpStream,
        deadline: CustodyReadDeadline,
        peer_leaf: &str,
    ) -> Result<Self, ReaderError> {
        stream
            .set_nonblocking(false)
            .map_err(|_| ReaderError::Read)?;
        connection.set_buffer_limit(Some(16_384));
        let mut socket = DeadlineSocket { stream, deadline };
        while connection.is_handshaking() {
            connection
                .complete_io(&mut socket)
                .map_err(|_| ReaderError::Read)?;
        }
        deadline.remaining().map_err(|_| ReaderError::Read)?;
        let certificates = connection.peer_certificates().ok_or(ReaderError::Binding)?;
        if connection.alpn_protocol() != Some(b"http/1.1")
            || certificates
                .first()
                .map(|c| super::raw_sha256(c.as_ref()))
                .as_deref()
                != Some(peer_leaf)
        {
            return Err(ReaderError::Binding);
        }
        Ok(Self { connection, socket })
    }
    pub(super) fn collect(
        &mut self,
        body_length: impl FnOnce(&[u8]) -> Result<usize, ReaderError>,
        eof: bool,
    ) -> Result<Vec<u8>, ReaderError> {
        let mut bytes = Vec::with_capacity(http::HEADER_LIMIT);
        while !bytes.ends_with(b"\r\n\r\n") {
            if bytes.len() == http::HEADER_LIMIT {
                return Err(ReaderError::Format);
            }
            let mut byte = [0];
            self.read_exact(&mut byte).map_err(|_| ReaderError::Read)?;
            bytes.push(byte[0]);
        }
        let size = body_length(&bytes)?;
        let end = bytes.len().checked_add(size).ok_or(ReaderError::Format)?;
        bytes
            .try_reserve_exact(size)
            .map_err(|_| ReaderError::Format)?;
        let start = bytes.len();
        bytes.resize(end, 0);
        self.read_exact(&mut bytes[start..])
            .map_err(|_| ReaderError::Read)?;
        if eof {
            let mut extra = [0];
            if self.read(&mut extra).map_err(|_| ReaderError::Read)? != 0 {
                return Err(ReaderError::Format);
            }
        }
        self.socket.remaining().map_err(|_| ReaderError::Read)?;
        Ok(bytes)
    }
    pub(super) fn finish(&mut self) -> Result<(), ReaderError> {
        self.connection.send_close_notify();
        self.flush().map_err(|_| ReaderError::Read)
    }
}
impl Read for Channel {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            self.socket.remaining()?;
            match self.connection.reader().read(out) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.connection.complete_io(&mut self.socket)?;
                }
                value => return value,
            }
        }
    }
}
impl Write for Channel {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket.remaining()?;
        let n = self.connection.writer().write(bytes)?;
        self.flush()?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.remaining()?;
        while self.connection.wants_write() {
            self.connection.complete_io(&mut self.socket)?;
        }
        Ok(())
    }
}
