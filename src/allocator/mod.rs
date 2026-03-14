//! Memory allocator module exports.
//!
//! - `buddy`: power-of-two allocator for page/block management.
//! - `slab`: fixed-size object allocator for high-frequency allocations.

pub mod buddy;
pub mod slab;

pub use buddy::BuddyAllocator;
pub use slab::SlabAllocator;
