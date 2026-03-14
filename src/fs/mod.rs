//! Filesystem support surfaces.

pub mod sosfs;

#[cfg(feature = "crypto")]
pub use sosfs::derive_default_passkey;
pub use sosfs::{
    build_superblock, fsck_superblock_pair, probe_sosfs_superblock, validate_superblock,
    SosfsFsckIssue, SosfsFsckReport, SosfsFsckStatus, SosfsSuperblockInfo, SOSFS_BLOCK_SIZE,
    SOSFS_DEFAULT_PASSKEY, SOSFS_FLAG_ENCRYPTION_REQUIRED, SOSFS_FLAG_VERSIONING_REQUIRED,
    SOSFS_MAGIC,
};
