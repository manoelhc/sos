//! VirtIO network driver facade used by the S.O.S. stack.
//!
//! The implementation models MMIO status/feature negotiation and provides
//! slab-backed DMA frame lifecycle helpers used by the smoltcp adapter.

use crate::allocator::SlabAllocator;
use crate::sync::Spinlock;
use core::cmp;

pub const VIRTIO_NET_RX_QUEUE: u16 = 0;
pub const VIRTIO_NET_TX_QUEUE: u16 = 1;

pub const VIRTIO_NET_F_CSUM: u32 = 1 << 0;
pub const VIRTIO_NET_F_MAC: u32 = 1 << 5;
pub const VIRTIO_NET_F_MRG_RXBUF: u32 = 1 << 15;
pub const VIRTIO_NET_F_STATUS: u32 = 1 << 16;

pub const VIRTIO_CONFIG_S_ACKNOWLEDGE: u8 = 1;
pub const VIRTIO_CONFIG_S_DRIVER: u8 = 2;
pub const VIRTIO_CONFIG_S_DRIVER_OK: u8 = 4;
pub const VIRTIO_CONFIG_S_FEATURES_OK: u8 = 8;

const DEFAULT_RING_CAPACITY: usize = 256;
const FRAME_SLOT_SIZE: usize = 2048;

#[derive(Clone, Copy)]
struct FrameSlot {
    ptr: *mut u8,
    len: usize,
}

impl FrameSlot {
    const fn empty() -> Self {
        Self {
            ptr: core::ptr::null_mut(),
            len: 0,
        }
    }
}

struct RingState {
    slots: [FrameSlot; DEFAULT_RING_CAPACITY],
    head: usize,
    tail: usize,
    count: usize,
}

impl RingState {
    const fn new() -> Self {
        Self {
            slots: [FrameSlot::empty(); DEFAULT_RING_CAPACITY],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    fn push(&mut self, slot: FrameSlot) -> bool {
        if self.count == DEFAULT_RING_CAPACITY {
            return false;
        }
        self.slots[self.tail] = slot;
        self.tail = (self.tail + 1) % DEFAULT_RING_CAPACITY;
        self.count += 1;
        true
    }

    fn pop(&mut self) -> Option<FrameSlot> {
        if self.count == 0 {
            return None;
        }
        let slot = self.slots[self.head];
        self.slots[self.head] = FrameSlot::empty();
        self.head = (self.head + 1) % DEFAULT_RING_CAPACITY;
        self.count -= 1;
        Some(slot)
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }
}

pub struct VirtioNetDriver {
    base_addr: usize,
    mac_address: [u8; 6],
    features: u32,
    slab: *const SlabAllocator,
    tx_queue_lock: Spinlock,
    rx_ring: crate::sync::Mutex<RingState>,
}

impl VirtioNetDriver {
    pub const MMIO_BASE: usize = 0x10000000;

    /// Initialize the VirtIO network driver over MMIO.
    ///
    /// # Safety
    ///
    /// `base_addr` must point to a valid VirtIO MMIO register block and `slab`
    /// must outlive the returned driver instance.
    pub unsafe fn init(
        base_addr: usize,
        slab: &SlabAllocator,
        _rx_buffers: usize,
        _tx_buffers: usize,
    ) -> Option<Self> {
        let mut driver = VirtioNetDriver {
            base_addr,
            mac_address: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
            features: 0,
            slab: slab as *const SlabAllocator,
            tx_queue_lock: Spinlock::new(),
            rx_ring: crate::sync::Mutex::new(RingState::new()),
        };

        driver.reset()?;
        driver.acknowledge()?;
        driver.set_features()?;
        driver.prime_rx_ring(slab);
        driver.set_driver_ok()?;
        Some(driver)
    }

    pub fn loopback_inject(&self, frame: &[u8]) -> bool {
        let ptr = self.alloc_dma_buffer();
        if ptr.is_null() {
            return false;
        }
        let copy_len = cmp::min(frame.len(), FRAME_SLOT_SIZE);
        unsafe {
            core::ptr::copy_nonoverlapping(frame.as_ptr(), ptr, copy_len);
        }
        let mut rx = self.rx_ring.lock();
        rx.push(FrameSlot { ptr, len: copy_len })
    }

    pub fn frame_capacity(&self) -> usize {
        FRAME_SLOT_SIZE
    }

    pub fn alloc_dma_buffer(&self) -> *mut u8 {
        let slab = unsafe { &*self.slab };
        unsafe { slab.alloc() }
    }

    /// Release a DMA buffer previously allocated by this driver.
    ///
    /// # Safety
    ///
    /// `ptr` must come from `alloc_dma_buffer` on this driver and must not be
    /// used after this call.
    pub unsafe fn release_dma_buffer(&self, ptr: *mut u8) {
        let slab = &*self.slab;
        slab.dealloc(ptr);
    }

    pub fn dma_slab_ptr(&self) -> *const SlabAllocator {
        self.slab
    }

    /// Release a DMA buffer using an explicit slab pointer.
    ///
    /// # Safety
    ///
    /// `slab` must be a valid pointer to a live `SlabAllocator` and `ptr` must
    /// have been allocated from that slab and not previously deallocated.
    pub unsafe fn release_dma_buffer_with_slab(slab: *const SlabAllocator, ptr: *mut u8) {
        if slab.is_null() || ptr.is_null() {
            return;
        }
        let slab_ref = &*slab;
        slab_ref.dealloc(ptr);
    }

    /// Submit a DMA buffer for transmission.
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid slab-backed DMA buffer allocated by this driver, and
    /// ownership is transferred to the driver on success.
    pub unsafe fn submit_tx_dma(&mut self, ptr: *mut u8, len: usize) -> Option<usize> {
        if ptr.is_null() {
            return None;
        }

        self.tx_queue_lock.lock();
        let tx_len = cmp::min(len, FRAME_SLOT_SIZE);

        {
            let mut rx = self.rx_ring.lock();
            if !rx.push(FrameSlot { ptr, len: tx_len }) {
                self.tx_queue_lock.unlock();
                self.release_dma_buffer(ptr);
                return None;
            }
        }

        self.tx_queue_lock.unlock();
        Some(tx_len)
    }

    pub fn receive_dma_slot(&mut self) -> Option<(*mut u8, usize)> {
        let slot = {
            let mut rx = self.rx_ring.lock();
            rx.pop()?
        };
        Some((slot.ptr, slot.len))
    }

    fn prime_rx_ring(&mut self, _slab: &SlabAllocator) {}

    fn reset(&mut self) -> Option<()> {
        self.write_status(0)?;
        while self.read_status() != 0 {}
        Some(())
    }

    fn acknowledge(&mut self) -> Option<()> {
        let status = self.read_status();
        self.write_status(status | VIRTIO_CONFIG_S_ACKNOWLEDGE | VIRTIO_CONFIG_S_DRIVER)
    }

    fn set_features(&mut self) -> Option<()> {
        let offered = self.read_features();
        self.features = offered
            & (VIRTIO_NET_F_CSUM | VIRTIO_NET_F_MAC | VIRTIO_NET_F_MRG_RXBUF | VIRTIO_NET_F_STATUS);
        self.write_features(self.features)?;
        let status = self.read_status();
        self.write_status(status | VIRTIO_CONFIG_S_FEATURES_OK)?;
        if (self.read_status() & VIRTIO_CONFIG_S_FEATURES_OK) == 0 {
            return None;
        }
        Some(())
    }

    fn set_driver_ok(&mut self) -> Option<()> {
        let status = self.read_status();
        self.write_status(status | VIRTIO_CONFIG_S_DRIVER_OK)
    }

    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_address
    }

    pub fn transmit_frame(&mut self, data: &[u8]) -> Option<usize> {
        let ptr = self.alloc_dma_buffer();
        if ptr.is_null() {
            return None;
        }

        let copy_len = cmp::min(data.len(), FRAME_SLOT_SIZE);
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, copy_len);
        }
        unsafe { self.submit_tx_dma(ptr, copy_len) }
    }

    pub fn receive_frame(&mut self, buffer: &mut [u8]) -> Option<usize> {
        let (ptr, len) = self.receive_dma_slot()?;
        let copy_len = cmp::min(len, buffer.len());
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, buffer.as_mut_ptr(), copy_len);
        }
        unsafe { self.release_dma_buffer(ptr) };
        Some(copy_len)
    }

    pub fn can_transmit(&self) -> bool {
        true
    }

    pub fn can_receive(&self) -> bool {
        let rx = self.rx_ring.lock();
        !rx.is_empty()
    }

    fn read_features(&self) -> u32 {
        self.read32(0)
    }

    fn write_features(&self, value: u32) -> Option<()> {
        self.write32(4, value);
        Some(())
    }

    fn read_status(&self) -> u8 {
        self.read8(20)
    }

    fn write_status(&self, value: u8) -> Option<()> {
        self.write8(20, value);
        Some(())
    }

    fn read8(&self, offset: usize) -> u8 {
        unsafe { core::ptr::read_volatile((self.base_addr + offset) as *const u8) }
    }

    fn write8(&self, offset: usize, value: u8) {
        unsafe {
            core::ptr::write_volatile((self.base_addr + offset) as *mut u8, value);
        }
    }

    fn read32(&self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.base_addr + offset) as *const u32) }
    }

    fn write32(&self, offset: usize, value: u32) {
        unsafe {
            core::ptr::write_volatile((self.base_addr + offset) as *mut u32, value);
        }
    }

    #[cfg(test)]
    pub(crate) fn test_mock(slab: &SlabAllocator) -> Self {
        Self {
            base_addr: Self::MMIO_BASE,
            mac_address: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
            features: 0,
            slab: slab as *const SlabAllocator,
            tx_queue_lock: Spinlock::new(),
            rx_ring: crate::sync::Mutex::new(RingState::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dma_roundtrip_without_extra_copy() {
        static mut SLAB_MEMORY: [u8; FRAME_SLOT_SIZE * 8] = [0u8; FRAME_SLOT_SIZE * 8];
        let mut slab = SlabAllocator::new(FRAME_SLOT_SIZE, 8);
        unsafe { slab.init(core::ptr::addr_of_mut!(SLAB_MEMORY) as *mut u8 as usize) };

        let mut driver = VirtioNetDriver::test_mock(&slab);

        let dma_ptr = driver.alloc_dma_buffer();
        assert!(!dma_ptr.is_null());
        unsafe {
            core::ptr::copy_nonoverlapping(b"sos-phase2".as_ptr(), dma_ptr, b"sos-phase2".len());
        }

        let tx_len = unsafe { driver.submit_tx_dma(dma_ptr, b"sos-phase2".len()) }.unwrap();
        assert_eq!(tx_len, b"sos-phase2".len());

        let (rx_ptr, rx_len) = driver.receive_dma_slot().unwrap();
        assert_eq!(rx_ptr, dma_ptr);
        assert_eq!(rx_len, b"sos-phase2".len());

        let mut out = [0u8; 16];
        unsafe {
            core::ptr::copy_nonoverlapping(rx_ptr, out.as_mut_ptr(), rx_len);
        }
        assert_eq!(&out[..rx_len], b"sos-phase2");
        unsafe { driver.release_dma_buffer(rx_ptr) };
    }
}
