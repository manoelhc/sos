//! Networking subsystem surface.
//!
//! This module groups:
//! - VirtIO network driver (`virtio`),
//! - smoltcp integration and socket orchestration (`stack`),
//! - optional TLS integration wrappers (`tls`).

#[cfg(feature = "std")]
pub mod readiness;
pub mod stack;
pub mod tls;
pub mod virtio;

#[cfg(feature = "std")]
pub use readiness::{ReadinessCheck, ReadinessStatus, ReadinessSuite};
#[cfg(feature = "tls13")]
pub use stack::{NetworkIoError, NetworkStackIo};
pub use stack::{
    NetworkResources, NetworkStack, TcpSocketConfig, TcpWindowScaler, VirtioRxToken, VirtioTxToken,
};
pub use tls::{default_client_config, TlsHandler, TlsState, TLS_MAX_FRAME_SIZE};
pub use virtio::VirtioNetDriver;
