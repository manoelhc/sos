//! TLS integration layer.
//!
//! When `tls13` is enabled this wraps `embedded-tls` into a small stateful
//! handler API used by the network stack integration tests and runtime code.

#[cfg(feature = "tls13")]
mod enabled {
    use embedded_tls::blocking::TlsConnection;
    use embedded_tls::Aes128GcmSha256;
    use embedded_tls::TlsConfig;
    use embedded_tls::TlsContext;
    use embedded_tls::UnsecureProvider;
    use rand_core::{CryptoRng, CryptoRngCore};

    pub use embedded_tls::TlsError;

    pub const TLS_MAX_FRAME_SIZE: usize = 16_384;

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum TlsState {
        Initial,
        HandshakeInProgress,
        Connected,
        Closed,
        Error,
    }

    pub struct TlsHandler<'a, Socket, Rng>
    where
        Socket: embedded_io::Read + embedded_io::Write + 'a,
        Rng: CryptoRngCore + CryptoRng + 'a,
    {
        conn: Option<TlsConnection<'a, Socket, Aes128GcmSha256>>,
        config: TlsConfig<'a>,
        rng: Option<Rng>,
        state: TlsState,
    }

    impl<'a, Socket, Rng> TlsHandler<'a, Socket, Rng>
    where
        Socket: embedded_io::Read + embedded_io::Write + 'a,
        Rng: CryptoRngCore + CryptoRng + 'a,
    {
        pub fn new(config: TlsConfig<'a>, rng: Rng) -> Self {
            Self {
                conn: None,
                config,
                rng: Some(rng),
                state: TlsState::Initial,
            }
        }

        pub fn state(&self) -> TlsState {
            self.state
        }

        pub fn open(
            &mut self,
            socket: Socket,
            read_buf: &'a mut [u8],
            write_buf: &'a mut [u8],
        ) -> Result<(), TlsError> {
            self.state = TlsState::HandshakeInProgress;
            let mut conn = TlsConnection::new(socket, read_buf, write_buf);
            let rng = self.rng.take().ok_or(TlsError::InternalError)?;
            let ctx = TlsContext::new(&self.config, UnsecureProvider::new::<Aes128GcmSha256>(rng));
            match conn.open(ctx) {
                Ok(()) => {
                    self.state = TlsState::Connected;
                    self.conn = Some(conn);
                    Ok(())
                }
                Err(err) => {
                    self.state = TlsState::Error;
                    Err(err)
                }
            }
        }

        pub fn write(&mut self, data: &[u8]) -> Result<usize, TlsError> {
            let conn = self.conn.as_mut().ok_or(TlsError::MissingHandshake)?;
            conn.write(data)
        }

        pub fn read(&mut self, out: &mut [u8]) -> Result<usize, TlsError> {
            let conn = self.conn.as_mut().ok_or(TlsError::MissingHandshake)?;
            conn.read(out)
        }

        pub fn flush(&mut self) -> Result<(), TlsError> {
            let conn = self.conn.as_mut().ok_or(TlsError::MissingHandshake)?;
            conn.flush()
        }
    }

    pub fn default_client_config<'a>(server_name: &'a str) -> TlsConfig<'a> {
        TlsConfig::new().with_server_name(server_name)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use embedded_io::{Error as _, ErrorType, Read, Write};
        use rcgen::generate_simple_self_signed;
        use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        use rustls::{ServerConfig, ServerConnection};
        use std::io;
        use std::net::{TcpListener, TcpStream};
        use std::sync::Arc;

        #[derive(Clone)]
        struct DeterministicRng(u64);

        impl CryptoRng for DeterministicRng {}

        impl rand_core::RngCore for DeterministicRng {
            fn next_u32(&mut self) -> u32 {
                (self.next_u64() >> 32) as u32
            }

            fn next_u64(&mut self) -> u64 {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                self.0
            }

            fn fill_bytes(&mut self, dest: &mut [u8]) {
                for chunk in dest.chunks_mut(8) {
                    let bytes = self.next_u64().to_le_bytes();
                    let len = core::cmp::min(chunk.len(), bytes.len());
                    chunk[..len].copy_from_slice(&bytes[..len]);
                }
            }

            fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
                self.fill_bytes(dest);
                Ok(())
            }
        }

        #[derive(Debug)]
        struct StdIoError(io::ErrorKind);

        impl core::fmt::Display for StdIoError {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{:?}", self.0)
            }
        }

        impl core::error::Error for StdIoError {}

        impl embedded_io::Error for StdIoError {
            fn kind(&self) -> embedded_io::ErrorKind {
                match self.0 {
                    io::ErrorKind::NotFound => embedded_io::ErrorKind::NotFound,
                    io::ErrorKind::PermissionDenied => embedded_io::ErrorKind::PermissionDenied,
                    io::ErrorKind::ConnectionRefused => embedded_io::ErrorKind::ConnectionRefused,
                    io::ErrorKind::ConnectionReset => embedded_io::ErrorKind::ConnectionReset,
                    io::ErrorKind::ConnectionAborted => embedded_io::ErrorKind::ConnectionAborted,
                    io::ErrorKind::NotConnected => embedded_io::ErrorKind::NotConnected,
                    io::ErrorKind::AddrInUse => embedded_io::ErrorKind::AddrInUse,
                    io::ErrorKind::AddrNotAvailable => embedded_io::ErrorKind::AddrNotAvailable,
                    io::ErrorKind::BrokenPipe => embedded_io::ErrorKind::BrokenPipe,
                    io::ErrorKind::AlreadyExists => embedded_io::ErrorKind::AlreadyExists,
                    io::ErrorKind::InvalidInput => embedded_io::ErrorKind::InvalidInput,
                    io::ErrorKind::InvalidData => embedded_io::ErrorKind::InvalidData,
                    io::ErrorKind::TimedOut => embedded_io::ErrorKind::TimedOut,
                    io::ErrorKind::Interrupted => embedded_io::ErrorKind::Interrupted,
                    io::ErrorKind::Unsupported => embedded_io::ErrorKind::Unsupported,
                    io::ErrorKind::OutOfMemory => embedded_io::ErrorKind::OutOfMemory,
                    io::ErrorKind::WriteZero => embedded_io::ErrorKind::WriteZero,
                    _ => embedded_io::ErrorKind::Other,
                }
            }
        }

        struct StdIoSocket {
            inner: TcpStream,
        }

        impl StdIoSocket {
            fn new(inner: TcpStream) -> Self {
                Self { inner }
            }
        }

        impl ErrorType for StdIoSocket {
            type Error = StdIoError;
        }

        impl Read for StdIoSocket {
            fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
                std::io::Read::read(&mut self.inner, buf).map_err(|e| StdIoError(e.kind()))
            }
        }

        impl Write for StdIoSocket {
            fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
                std::io::Write::write(&mut self.inner, buf).map_err(|e| StdIoError(e.kind()))
            }

            fn flush(&mut self) -> Result<(), Self::Error> {
                std::io::Write::flush(&mut self.inner).map_err(|e| StdIoError(e.kind()))
            }
        }

        #[test]
        fn test_tls_handler_open_success_with_mock_server_transcript() {
            let cert_key = generate_simple_self_signed(vec!["localhost".to_string()])
                .expect("cert generation");
            let cert = cert_key.cert.der().clone();
            let key =
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert_key.key_pair.serialize_der()));

            let server_cfg = ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)
                .expect("server config");
            let server_cfg = Arc::new(server_cfg);

            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let addr = listener.local_addr().expect("local addr");

            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut conn = ServerConnection::new(server_cfg).expect("server conn");
                while conn.is_handshaking() {
                    let _ = conn.complete_io(&mut stream).expect("server handshake io");
                }
            });

            let stream = TcpStream::connect(addr).expect("connect");
            stream.set_nodelay(true).expect("set nodelay");

            let socket = StdIoSocket::new(stream);
            let mut read_buf = [0u8; TLS_MAX_FRAME_SIZE];
            let mut write_buf = [0u8; TLS_MAX_FRAME_SIZE];
            let mut handler =
                TlsHandler::new(default_client_config("localhost"), DeterministicRng(11));

            let open = handler.open(socket, &mut read_buf, &mut write_buf);
            assert!(
                open.is_ok(),
                "handshake should succeed, got: {:?}",
                open.err()
            );
            assert_eq!(handler.state(), TlsState::Connected);

            server.join().expect("server thread");
        }

        #[test]
        fn test_tls_handler_reports_missing_handshake_after_failed_open() {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let addr = listener.local_addr().expect("local addr");

            let server = std::thread::spawn(move || {
                let (_stream, _) = listener.accept().expect("accept");
            });

            let stream = TcpStream::connect(addr).expect("connect");
            let socket = StdIoSocket::new(stream);
            let mut read_buf = [0u8; TLS_MAX_FRAME_SIZE];
            let mut write_buf = [0u8; TLS_MAX_FRAME_SIZE];
            let mut handler =
                TlsHandler::new(default_client_config("localhost"), DeterministicRng(13));

            let _ = handler.open(socket, &mut read_buf, &mut write_buf);
            let write_res = handler.write(b"hello");
            assert!(write_res.is_err());
            let kind = write_res.err().expect("err").kind();
            assert_eq!(kind, embedded_io::ErrorKind::Other);

            server.join().expect("server thread");
        }
    }
}

#[cfg(not(feature = "tls13"))]
mod disabled {
    pub const TLS_MAX_FRAME_SIZE: usize = 16_384;

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum TlsState {
        Initial,
        HandshakeInProgress,
        Connected,
        Closed,
        Error,
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum TlsError {
        Disabled,
    }

    pub struct TlsHandler;

    impl TlsHandler {
        pub fn new() -> Self {
            Self
        }

        pub fn state(&self) -> TlsState {
            TlsState::Initial
        }

        pub fn open(&mut self) -> Result<(), TlsError> {
            Err(TlsError::Disabled)
        }

        pub fn write(&mut self, _data: &[u8]) -> Result<usize, TlsError> {
            Err(TlsError::Disabled)
        }

        pub fn read(&mut self, _out: &mut [u8]) -> Result<usize, TlsError> {
            Err(TlsError::Disabled)
        }

        pub fn flush(&mut self) -> Result<(), TlsError> {
            Err(TlsError::Disabled)
        }
    }

    impl Default for TlsHandler {
        fn default() -> Self {
            Self::new()
        }
    }

    pub fn default_client_config(_server_name: &str) {}
}

#[cfg(not(feature = "tls13"))]
pub use disabled::*;
#[cfg(feature = "tls13")]
pub use enabled::*;
