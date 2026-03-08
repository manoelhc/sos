//! Synchronization Primitives
//!
//! Spinlocks and atomic operations for bare-metal environments.
//! Implements interrupt-safe spinlocks to prevent deadlock.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct Spinlock {
    locked: AtomicBool,
    irq_flags: AtomicUsize,
}

impl Spinlock {
    pub const fn new() -> Self {
        Spinlock {
            locked: AtomicBool::new(false),
            irq_flags: AtomicUsize::new(0),
        }
    }

    #[inline]
    fn disable_interrupts() -> usize {
        #[cfg(test)]
        {
            return 0x200;
        }

        #[cfg(not(test))]
        {
            let flags: usize;
            unsafe {
                core::arch::asm!("pushfq; pop {}; cli", out(reg) flags, options(nostack));
            }
            flags & 0x200
        }
    }

    #[inline]
    fn restore_interrupts(flags: usize) {
        #[cfg(test)]
        {
            let _ = flags;
            return;
        }

        #[cfg(not(test))]
        {
            if flags & 0x200 != 0 {
                unsafe {
                    core::arch::asm!("sti", options(nostack));
                }
            }
        }
    }

    pub fn lock(&self) {
        let mut saved_flags = Self::disable_interrupts();
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            Self::restore_interrupts(saved_flags);
            core::hint::spin_loop();
            saved_flags = Self::disable_interrupts();
        }
        self.irq_flags.store(saved_flags, Ordering::Release);
    }

    pub fn unlock(&self) {
        let saved_flags = self.irq_flags.swap(0, Ordering::AcqRel);
        self.locked.store(false, Ordering::Release);
        Self::restore_interrupts(saved_flags);
    }

    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }
}

impl Default for Spinlock {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Mutex<T> {
    lock: Spinlock,
    data: UnsafeCell<T>,
}

impl<T> Mutex<T> {
    pub const fn new(data: T) -> Self {
        Mutex {
            lock: Spinlock::new(),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.lock.lock();
        MutexGuard { mutex: self }
    }
}

pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex.lock.unlock();
    }
}

impl<'a, T> core::ops::Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T> core::ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

pub struct AtomicSlabBitmap {
    bitmap: AtomicUsize,
    max_bits: usize,
}

impl AtomicSlabBitmap {
    pub const fn new(max_bits: usize) -> Self {
        AtomicSlabBitmap {
            bitmap: AtomicUsize::new(0),
            max_bits,
        }
    }

    pub fn try_set_bit(&self, index: usize) -> bool {
        if index >= self.max_bits {
            return false;
        }
        let mask = 1usize << index;
        self.bitmap.fetch_or(mask, Ordering::AcqRel) & mask == 0
    }

    pub fn try_unset_bit(&self, index: usize) -> bool {
        if index >= self.max_bits {
            return false;
        }
        let mask = 1usize << index;
        let old = self.bitmap.fetch_and(!mask, Ordering::AcqRel);
        old & mask != 0
    }

    pub fn is_set(&self, index: usize) -> bool {
        if index >= self.max_bits {
            return false;
        }
        self.bitmap.load(Ordering::Acquire) & (1usize << index) != 0
    }

    pub fn find_free(&self) -> Option<usize> {
        let bits = self.bitmap.load(Ordering::Acquire);
        (0..self.max_bits).find(|&i| bits & (1usize << i) == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinlock_new() {
        let lock = Spinlock::new();
        assert!(!lock.is_locked());
    }

    #[test]
    fn test_spinlock_lock_unlock() {
        let lock = Spinlock::new();
        lock.lock();
        assert!(lock.is_locked());
        lock.unlock();
        assert!(!lock.is_locked());
    }

    #[test]
    fn test_mutex_new() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock();
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_mutex_mutable_access() {
        let mutex = Mutex::new(0);
        {
            let mut guard = mutex.lock();
            *guard = 42;
        }
        assert_eq!(*mutex.lock(), 42);
    }

    #[test]
    fn test_atomic_slab_bitmap_new() {
        let bitmap = AtomicSlabBitmap::new(64);
        assert!(!bitmap.is_set(0));
    }

    #[test]
    fn test_atomic_slab_bitmap_set() {
        let bitmap = AtomicSlabBitmap::new(64);
        assert!(bitmap.try_set_bit(5));
        assert!(bitmap.is_set(5));
    }

    #[test]
    fn test_atomic_slab_bitmap_find_free() {
        let bitmap = AtomicSlabBitmap::new(64);
        assert_eq!(bitmap.find_free(), Some(0));
        let _ = bitmap.try_set_bit(0);
        assert_eq!(bitmap.find_free(), Some(1));
    }
}
