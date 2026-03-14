//! smoltcp integration layer for S.O.S.
//!
//! Responsibilities:
//! - adapts `VirtioNetDriver` to `smoltcp::phy::Device`,
//! - provides DMA-backed RX/TX token handling,
//! - configures a TCP socket surface with RTT/window tuning,
//! - exposes `NetworkStackIo` (`embedded-io`) for TLS wiring.

use crate::allocator::SlabAllocator;
use crate::network::virtio::VirtioNetDriver;
use smoltcp::iface::{Config as IfaceConfig, Interface, SocketHandle, SocketSet, SocketStorage};
use smoltcp::phy::{
    Checksum, ChecksumCapabilities, Device, DeviceCapabilities, Medium, RxToken, TxToken,
};
use smoltcp::socket::tcp;
use smoltcp::storage::RingBuffer;
use smoltcp::time::{Duration, Instant};
use smoltcp::wire::{
    EthernetAddress, HardwareAddress, IpAddress, IpCidr, IpListenEndpoint, Ipv4Address,
};

pub const TCP_WINDOW_SCALE: u8 = 7;
pub const DEFAULT_MTU: usize = 1500;
pub const TCP_RX_BUF_SIZE: usize = 256 * 1024;
pub const TCP_TX_BUF_SIZE: usize = 256 * 1024;

pub struct NetworkResources<'a> {
    pub sockets: &'a mut [SocketStorage<'a>],
    pub tcp_rx: &'a mut [u8],
    pub tcp_tx: &'a mut [u8],
}

pub struct VirtioRxToken {
    ptr: *mut u8,
    len: usize,
    slab: *const SlabAllocator,
}

impl RxToken for VirtioRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let view = unsafe { core::slice::from_raw_parts(self.ptr, self.len) };
        let result = f(view);
        unsafe { VirtioNetDriver::release_dma_buffer_with_slab(self.slab, self.ptr) };
        result
    }
}

pub struct VirtioTxToken<'a> {
    device: &'a mut VirtioNetDriver,
}

impl<'a> TxToken for VirtioTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let ptr = self.device.alloc_dma_buffer();
        if ptr.is_null() {
            let mut frame = [0u8; DEFAULT_MTU];
            let cap = core::cmp::min(len, frame.len());
            let result = f(&mut frame[..cap]);
            let _ = self.device.transmit_frame(&frame[..cap]);
            return result;
        }

        let cap = core::cmp::min(len, self.device.frame_capacity());
        let result = {
            let frame = unsafe { core::slice::from_raw_parts_mut(ptr, cap) };
            f(frame)
        };
        if unsafe { self.device.submit_tx_dma(ptr, cap) }.is_none() {
            unsafe { self.device.release_dma_buffer(ptr) };
        }
        result
    }
}

impl Device for VirtioNetDriver {
    type RxToken<'a>
        = VirtioRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = VirtioTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if !self.can_receive() {
            return None;
        }
        let (ptr, len) = self.receive_dma_slot()?;
        Some((
            VirtioRxToken {
                ptr,
                len,
                slab: self.dma_slab_ptr(),
            },
            VirtioTxToken { device: self },
        ))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        if !self.can_transmit() {
            return None;
        }
        Some(VirtioTxToken { device: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = DEFAULT_MTU;
        caps.medium = Medium::Ethernet;
        caps.max_burst_size = Some(1);
        let mut checksum = ChecksumCapabilities::ignored();
        checksum.ipv4 = Checksum::Tx;
        checksum.udp = Checksum::Tx;
        checksum.tcp = Checksum::Tx;
        checksum.icmpv4 = Checksum::None;
        caps.checksum = checksum;
        caps
    }
}

#[derive(Clone, Copy)]
pub struct TcpSocketConfig {
    pub local_port: u16,
    pub remote_addr: IpAddress,
    pub remote_port: u16,
    pub window_scale: u8,
    pub rx_buffer_size: usize,
    pub tx_buffer_size: usize,
}

impl TcpSocketConfig {
    pub fn new() -> Self {
        Self {
            local_port: 0,
            remote_addr: IpAddress::Ipv4(Ipv4Address::UNSPECIFIED),
            remote_port: 0,
            window_scale: TCP_WINDOW_SCALE,
            rx_buffer_size: TCP_RX_BUF_SIZE,
            tx_buffer_size: TCP_TX_BUF_SIZE,
        }
    }

    pub fn with_remote(mut self, addr: Ipv4Address, port: u16) -> Self {
        self.remote_addr = IpAddress::Ipv4(addr);
        self.remote_port = port;
        self
    }

    pub fn with_local_port(mut self, port: u16) -> Self {
        self.local_port = port;
        self
    }

    pub fn with_window_scale(mut self, scale: u8) -> Self {
        self.window_scale = core::cmp::min(scale, TcpWindowScaler::MAX_SCALE);
        self
    }
}

impl Default for TcpSocketConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub struct TcpWindowScaler;

impl TcpWindowScaler {
    pub const MAX_SCALE: u8 = 14;
    pub const DEFAULT_TARGET_THROUGHPUT_MBPS: usize = 1_000;

    pub fn recommended_window_bytes(rtt_ms: usize, throughput_mbps: usize) -> usize {
        let safe_rtt = core::cmp::max(rtt_ms, 1);
        let bits_per_second = throughput_mbps.saturating_mul(1_000_000);
        let bytes_per_second = bits_per_second / 8;
        let bytes_per_rtt = bytes_per_second.saturating_mul(safe_rtt) / 1_000;
        core::cmp::max(bytes_per_rtt, 65_535)
    }

    pub fn calibrate_buffers(rtt_ms: usize) -> (usize, usize, u8) {
        let target = Self::recommended_window_bytes(rtt_ms, Self::DEFAULT_TARGET_THROUGHPUT_MBPS);
        let aligned = target.next_power_of_two();
        let rx = core::cmp::max(aligned, 65_536);
        let tx = core::cmp::max(rx / 2, 65_536);
        let scale = Self::calculate_scale(rx);
        (rx, tx, scale)
    }

    pub fn calculate_scale(window_size: usize) -> u8 {
        let mut shift = 0u8;
        let mut current = 65_535usize;
        while current < window_size && shift < Self::MAX_SCALE {
            current <<= 1;
            shift += 1;
        }
        shift
    }
}

pub struct NetworkStack<'a> {
    iface: Interface,
    sockets: SocketSet<'a>,
    tcp_handle: SocketHandle,
}

#[cfg(feature = "tls13")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NetworkIoError {
    Send,
    Receive,
    WriteZero,
}

#[cfg(feature = "tls13")]
impl core::fmt::Display for NetworkIoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NetworkIoError::Send => write!(f, "send failed"),
            NetworkIoError::Receive => write!(f, "receive failed"),
            NetworkIoError::WriteZero => write!(f, "write zero"),
        }
    }
}

#[cfg(feature = "tls13")]
impl core::error::Error for NetworkIoError {}

#[cfg(feature = "tls13")]
impl embedded_io::Error for NetworkIoError {
    fn kind(&self) -> embedded_io::ErrorKind {
        match self {
            NetworkIoError::Send => embedded_io::ErrorKind::BrokenPipe,
            NetworkIoError::Receive => embedded_io::ErrorKind::ConnectionReset,
            NetworkIoError::WriteZero => embedded_io::ErrorKind::WriteZero,
        }
    }
}

#[cfg(feature = "tls13")]
pub struct NetworkStackIo<'stack, 'net> {
    stack: &'stack mut NetworkStack<'net>,
}

#[cfg(feature = "tls13")]
impl<'stack, 'net> embedded_io::ErrorType for NetworkStackIo<'stack, 'net> {
    type Error = NetworkIoError;
}

#[cfg(feature = "tls13")]
impl<'stack, 'net> embedded_io::Read for NetworkStackIo<'stack, 'net> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.stack.receive(buf).map_err(|_| NetworkIoError::Receive)
    }
}

#[cfg(feature = "tls13")]
impl<'stack, 'net> embedded_io::Write for NetworkStackIo<'stack, 'net> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let written = self.stack.send(buf).map_err(|_| NetworkIoError::Send)?;
        if !buf.is_empty() && written == 0 {
            return Err(NetworkIoError::WriteZero);
        }
        Ok(written)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl<'a> NetworkStack<'a> {
    pub fn required_resources() -> usize {
        1
    }

    pub fn new(
        device: &mut VirtioNetDriver,
        resources: NetworkResources<'a>,
        ip: Ipv4Address,
        gateway: Option<Ipv4Address>,
    ) -> Self {
        let hw = HardwareAddress::Ethernet(EthernetAddress::from_bytes(&device.mac_address()));
        let mut cfg = IfaceConfig::new(hw);
        cfg.random_seed = 0x5A5A_A5A5_DEAD_BEEF;

        let mut iface = Interface::new(cfg, device, Instant::from_millis(0));
        iface.update_ip_addrs(|addrs| {
            let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(ip), 24));
        });
        if let Some(gw) = gateway {
            let _ = iface.routes_mut().add_default_ipv4_route(gw);
        }

        let mut sockets = SocketSet::new(resources.sockets);
        let mut tcp_socket = tcp::Socket::new(
            RingBuffer::new(resources.tcp_rx),
            RingBuffer::new(resources.tcp_tx),
        );
        tcp_socket.set_nagle_enabled(false);
        tcp_socket.set_keep_alive(Some(Duration::from_millis(500)));
        tcp_socket.set_ack_delay(Some(Duration::from_millis(5)));

        let tcp_handle = sockets.add(tcp_socket);
        Self {
            iface,
            sockets,
            tcp_handle,
        }
    }

    pub fn poll(&mut self, timestamp: Instant, device: &mut VirtioNetDriver) {
        let _ = self.iface.poll(timestamp, device, &mut self.sockets);
    }

    pub fn listen(&mut self, port: u16) -> Result<(), tcp::ListenError> {
        let socket = self.sockets.get_mut::<tcp::Socket<'_>>(self.tcp_handle);
        socket.listen(port)
    }

    pub fn connect(
        &mut self,
        remote: (IpAddress, u16),
        local_port: u16,
    ) -> Result<(), tcp::ConnectError> {
        let socket = self.sockets.get_mut::<tcp::Socket<'_>>(self.tcp_handle);
        socket.connect(
            self.iface.context(),
            remote,
            IpListenEndpoint {
                addr: None,
                port: local_port,
            },
        )
    }

    pub fn close(&mut self) {
        let socket = self.sockets.get_mut::<tcp::Socket<'_>>(self.tcp_handle);
        socket.close();
    }

    pub fn is_connected(&mut self) -> bool {
        let socket = self.sockets.get_mut::<tcp::Socket<'_>>(self.tcp_handle);
        socket.is_active()
    }

    pub fn send(&mut self, data: &[u8]) -> Result<usize, tcp::SendError> {
        let socket = self.sockets.get_mut::<tcp::Socket<'_>>(self.tcp_handle);
        socket.send_slice(data)
    }

    pub fn receive(&mut self, data: &mut [u8]) -> Result<usize, tcp::RecvError> {
        let socket = self.sockets.get_mut::<tcp::Socket<'_>>(self.tcp_handle);
        socket.recv_slice(data)
    }

    pub fn apply_window_scaling(&mut self, config: TcpSocketConfig) {
        let socket = self.sockets.get_mut::<tcp::Socket<'_>>(self.tcp_handle);
        let configured_rx = core::cmp::max(config.rx_buffer_size, 65_535);
        let socket_scale = TcpWindowScaler::calculate_scale(socket.recv_capacity());
        let configured_scale = TcpWindowScaler::calculate_scale(configured_rx);
        let requested_scale = core::cmp::min(config.window_scale, TcpWindowScaler::MAX_SCALE);
        let effective_scale = core::cmp::min(
            requested_scale,
            core::cmp::max(socket_scale, configured_scale),
        );

        let ack_delay_ms = if effective_scale >= 6 { 2 } else { 5 };
        let timeout_ms = if effective_scale >= 6 { 400 } else { 250 };

        socket.set_ack_delay(Some(Duration::from_millis(ack_delay_ms)));
        socket.set_timeout(Some(Duration::from_millis(timeout_ms)));
        socket.set_keep_alive(Some(Duration::from_millis(500)));
    }

    pub fn apply_rtt_profile(&mut self, rtt_ms: usize) -> TcpSocketConfig {
        let (rx, tx, scale) = TcpWindowScaler::calibrate_buffers(rtt_ms);
        let config = TcpSocketConfig::new()
            .with_window_scale(scale)
            .with_buffers(rx, tx);
        self.apply_window_scaling(config);
        config
    }

    #[cfg(feature = "tls13")]
    pub fn tls_io(&mut self) -> NetworkStackIo<'_, 'a> {
        NetworkStackIo { stack: self }
    }
}

impl TcpSocketConfig {
    pub fn with_buffers(mut self, rx: usize, tx: usize) -> Self {
        self.rx_buffer_size = rx;
        self.tx_buffer_size = tx;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "tls13")]
    use crate::allocator::SlabAllocator;
    #[cfg(feature = "tls13")]
    use crate::network::tls::{default_client_config, TlsHandler, TlsState, TLS_MAX_FRAME_SIZE};
    #[cfg(feature = "tls13")]
    use crate::network::VirtioNetDriver;
    #[cfg(feature = "tls13")]
    use rand_core::CryptoRng;
    #[cfg(feature = "tls13")]
    use smoltcp::iface::SocketStorage;
    #[cfg(feature = "tls13")]
    use smoltcp::wire::Ipv4Address;

    #[test]
    fn test_window_scale_calculation() {
        assert_eq!(TcpWindowScaler::calculate_scale(65_535), 0);
        assert!(TcpWindowScaler::calculate_scale(256 * 1024) > 0);
    }

    #[test]
    fn test_rtt_buffer_calibration() {
        let (rx_low, tx_low, scale_low) = TcpWindowScaler::calibrate_buffers(2);
        let (rx_high, tx_high, scale_high) = TcpWindowScaler::calibrate_buffers(30);

        assert!(rx_low >= 65_536);
        assert!(tx_low >= 65_536);
        assert!(rx_high > rx_low);
        assert!(tx_high >= tx_low);
        assert!(scale_high >= scale_low);
    }

    #[cfg(feature = "tls13")]
    #[derive(Clone)]
    struct DeterministicRng(u64);

    #[cfg(feature = "tls13")]
    impl CryptoRng for DeterministicRng {}

    #[cfg(feature = "tls13")]
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

    #[cfg(feature = "tls13")]
    #[test]
    fn test_end_to_end_network_tls_integration() {
        let mut slab_memory = std::vec![0u8; 2048 * 32];
        let mut tcp_rx = std::vec![0u8; TCP_RX_BUF_SIZE];
        let mut tcp_tx = std::vec![0u8; TCP_TX_BUF_SIZE];
        let mut sockets: [SocketStorage<'_>; 1] = core::array::from_fn(|_| SocketStorage::EMPTY);
        let mut tls_read = std::vec![0u8; TLS_MAX_FRAME_SIZE];
        let mut tls_write = std::vec![0u8; TLS_MAX_FRAME_SIZE];

        let mut slab = SlabAllocator::new(2048, 32);
        unsafe { slab.init(slab_memory.as_mut_ptr() as usize) };

        let mut driver = VirtioNetDriver::test_mock(&slab);
        let resources = NetworkResources {
            sockets: &mut sockets,
            tcp_rx: &mut tcp_rx,
            tcp_tx: &mut tcp_tx,
        };

        let mut stack = NetworkStack::new(
            &mut driver,
            resources,
            Ipv4Address::new(10, 0, 0, 2),
            Some(Ipv4Address::new(10, 0, 0, 1)),
        );

        let tuned = stack.apply_rtt_profile(4);
        assert!(tuned.window_scale > 0);
        assert!(tuned.rx_buffer_size >= 65_536);
        assert!(tuned.tx_buffer_size >= 65_536);

        let io = stack.tls_io();
        let mut handler =
            TlsHandler::new(default_client_config("phase2.local"), DeterministicRng(7));
        let res = handler.open(io, &mut tls_read, &mut tls_write);

        assert!(res.is_err());
        assert_eq!(handler.state(), TlsState::Error);
    }

    #[test]
    fn test_recommended_window_monotonic_with_rtt() {
        let w1 = TcpWindowScaler::recommended_window_bytes(1, 1_000);
        let w5 = TcpWindowScaler::recommended_window_bytes(5, 1_000);
        let w20 = TcpWindowScaler::recommended_window_bytes(20, 1_000);

        assert!(w1 >= 65_535);
        assert!(w5 > w1);
        assert!(w20 > w5);
    }
}
