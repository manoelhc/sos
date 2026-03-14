//! sosfs on-disk recognition helpers.

#[cfg(feature = "crypto")]
use sha2::{Digest, Sha256};

pub const SOSFS_BLOCK_SIZE: usize = 4096;
pub const SOSFS_MAGIC: [u8; 8] = *b"SOSFS\0\0\0";
pub const SOSFS_FLAG_ENCRYPTION_REQUIRED: u64 = 0x1;
pub const SOSFS_FLAG_VERSIONING_REQUIRED: u64 = 0x2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SosfsFsckStatus {
    Clean,
    Warn,
    Corrupt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SosfsFsckIssue {
    BadMagic,
    BadVersion,
    BadChecksum,
    BadFlags,
    BadBlockSize,
    MirrorMismatch,
    GenerationMismatch,
}

pub const SOSFS_DEFAULT_PASSKEY: [u8; 32] = [
    0xc0, 0x94, 0x61, 0x06, 0xb7, 0x32, 0xf9, 0xf6, 0xae, 0x88, 0x91, 0x01, 0xab, 0x98, 0x7e, 0xd1,
    0xbb, 0xcf, 0xe3, 0xed, 0xa2, 0xad, 0x0a, 0x97, 0x1b, 0xe3, 0x15, 0x75, 0xad, 0x67, 0x68, 0x51,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SosfsSuperblockInfo {
    pub version_major: u16,
    pub version_minor: u16,
    pub flags: u64,
    pub active_generation: u64,
    pub fs_uuid: [u8; 16],
    pub fs_salt: [u8; 32],
}

#[cfg(feature = "crypto")]
pub fn derive_default_passkey() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"sos");
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn checksum32(data: &[u8]) -> u32 {
    let mut s = 0u32;
    for b in data {
        s = s.wrapping_add(*b as u32);
        s = s.rotate_left(3);
    }
    s
}

#[allow(clippy::too_many_arguments)]
pub fn build_superblock(
    version_major: u16,
    version_minor: u16,
    flags: u64,
    fs_uuid: [u8; 16],
    fs_salt: [u8; 32],
    active_generation: u64,
    wal_start_lba: u64,
    wal_blocks: u64,
    index_start_lba: u64,
    index_blocks: u64,
    data_start_lba: u64,
    data_blocks: u64,
    last_checkpoint_wal_seq: u64,
) -> [u8; SOSFS_BLOCK_SIZE] {
    let mut out = [0u8; SOSFS_BLOCK_SIZE];
    out[0..8].copy_from_slice(&SOSFS_MAGIC);
    out[8..10].copy_from_slice(&version_major.to_le_bytes());
    out[10..12].copy_from_slice(&version_minor.to_le_bytes());
    out[12..16].copy_from_slice(&(SOSFS_BLOCK_SIZE as u32).to_le_bytes());
    out[16..24].copy_from_slice(&flags.to_le_bytes());
    out[24..40].copy_from_slice(&fs_uuid);
    out[40..72].copy_from_slice(&fs_salt);
    out[72..80].copy_from_slice(&active_generation.to_le_bytes());
    out[80..88].copy_from_slice(&wal_start_lba.to_le_bytes());
    out[88..96].copy_from_slice(&wal_blocks.to_le_bytes());
    out[96..104].copy_from_slice(&index_start_lba.to_le_bytes());
    out[104..112].copy_from_slice(&index_blocks.to_le_bytes());
    out[112..120].copy_from_slice(&data_start_lba.to_le_bytes());
    out[120..128].copy_from_slice(&data_blocks.to_le_bytes());
    out[128..136].copy_from_slice(&last_checkpoint_wal_seq.to_le_bytes());
    let checksum = checksum32(&out[..SOSFS_BLOCK_SIZE - 4]);
    out[SOSFS_BLOCK_SIZE - 4..SOSFS_BLOCK_SIZE].copy_from_slice(&checksum.to_le_bytes());
    out
}

pub fn probe_sosfs_superblock(block: &[u8; SOSFS_BLOCK_SIZE]) -> Option<SosfsSuperblockInfo> {
    if block[0..8] != SOSFS_MAGIC {
        return None;
    }

    let version_major = u16::from_le_bytes([block[8], block[9]]);
    let version_minor = u16::from_le_bytes([block[10], block[11]]);
    let block_size = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
    if block_size as usize != SOSFS_BLOCK_SIZE {
        return None;
    }

    let expected = u32::from_le_bytes([
        block[SOSFS_BLOCK_SIZE - 4],
        block[SOSFS_BLOCK_SIZE - 3],
        block[SOSFS_BLOCK_SIZE - 2],
        block[SOSFS_BLOCK_SIZE - 1],
    ]);
    if checksum32(&block[..SOSFS_BLOCK_SIZE - 4]) != expected {
        return None;
    }

    let flags = u64::from_le_bytes([
        block[16], block[17], block[18], block[19], block[20], block[21], block[22], block[23],
    ]);

    if (flags & SOSFS_FLAG_ENCRYPTION_REQUIRED) == 0 {
        return None;
    }
    if (flags & SOSFS_FLAG_VERSIONING_REQUIRED) == 0 {
        return None;
    }

    let mut fs_uuid = [0u8; 16];
    fs_uuid.copy_from_slice(&block[24..40]);
    let mut fs_salt = [0u8; 32];
    fs_salt.copy_from_slice(&block[40..72]);
    let active_generation = u64::from_le_bytes([
        block[72], block[73], block[74], block[75], block[76], block[77], block[78], block[79],
    ]);

    Some(SosfsSuperblockInfo {
        version_major,
        version_minor,
        flags,
        active_generation,
        fs_uuid,
        fs_salt,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SosfsFsckReport {
    pub status: SosfsFsckStatus,
    pub issues: [Option<SosfsFsckIssue>; 4],
    pub issue_count: usize,
    pub mirror_valid: [bool; 2],
    pub info: Option<SosfsSuperblockInfo>,
}

impl SosfsFsckReport {
    pub fn clean() -> Self {
        Self {
            status: SosfsFsckStatus::Clean,
            issues: [None, None, None, None],
            issue_count: 0,
            mirror_valid: [true, true],
            info: None,
        }
    }

    pub fn with_issue(mut self, issue: SosfsFsckIssue) -> Self {
        if self.issue_count < 4 {
            self.issues[self.issue_count] = Some(issue);
            self.issue_count += 1;
        }
        self
    }
}

pub fn validate_superblock(
    block: &[u8; SOSFS_BLOCK_SIZE],
) -> Result<SosfsSuperblockInfo, SosfsFsckIssue> {
    if block[0..8] != SOSFS_MAGIC {
        return Err(SosfsFsckIssue::BadMagic);
    }

    let version_major = u16::from_le_bytes([block[8], block[9]]);
    let version_minor = u16::from_le_bytes([block[10], block[11]]);
    if version_major != 0 || version_minor != 1 {
        return Err(SosfsFsckIssue::BadVersion);
    }

    let block_size = u32::from_le_bytes([block[12], block[13], block[14], block[15]]);
    if block_size as usize != SOSFS_BLOCK_SIZE {
        return Err(SosfsFsckIssue::BadBlockSize);
    }

    let expected = u32::from_le_bytes([
        block[SOSFS_BLOCK_SIZE - 4],
        block[SOSFS_BLOCK_SIZE - 3],
        block[SOSFS_BLOCK_SIZE - 2],
        block[SOSFS_BLOCK_SIZE - 1],
    ]);
    if checksum32(&block[..SOSFS_BLOCK_SIZE - 4]) != expected {
        return Err(SosfsFsckIssue::BadChecksum);
    }

    let flags = u64::from_le_bytes([
        block[16], block[17], block[18], block[19], block[20], block[21], block[22], block[23],
    ]);

    if (flags & SOSFS_FLAG_ENCRYPTION_REQUIRED) == 0
        || (flags & SOSFS_FLAG_VERSIONING_REQUIRED) == 0
    {
        return Err(SosfsFsckIssue::BadFlags);
    }

    let fs_uuid = [0u8; 16];
    let fs_salt = [0u8; 32];
    let active_generation = u64::from_le_bytes([
        block[72], block[73], block[74], block[75], block[76], block[77], block[78], block[79],
    ]);

    Ok(SosfsSuperblockInfo {
        version_major,
        version_minor,
        flags,
        active_generation,
        fs_uuid,
        fs_salt,
    })
}

pub fn fsck_superblock_pair(
    sb0: &[u8; SOSFS_BLOCK_SIZE],
    sb1: &[u8; SOSFS_BLOCK_SIZE],
    strict: bool,
) -> SosfsFsckReport {
    let info0 = validate_superblock(sb0);
    let info1 = validate_superblock(sb1);

    let valid0 = info0.is_ok();
    let valid1 = info1.is_ok();

    if !valid0 && !valid1 {
        let mut report = SosfsFsckReport::clean();
        if let Err(e) = info0 {
            report = report.with_issue(e);
        }
        if let Err(e) = info1 {
            report = report.with_issue(e);
        }
        report.status = SosfsFsckStatus::Corrupt;
        report.mirror_valid = [false, false];
        return report;
    }

    match (info0, info1) {
        (Ok(i0), Ok(i1)) => {
            let mut report = SosfsFsckReport::clean();
            report.info = Some(i0);
            report.mirror_valid = [true, true];

            if i0.active_generation != i1.active_generation {
                report = report.with_issue(SosfsFsckIssue::GenerationMismatch);
                report.status = if strict {
                    SosfsFsckStatus::Corrupt
                } else {
                    SosfsFsckStatus::Warn
                };
            }

            if i0.fs_uuid != i1.fs_uuid {
                report = report.with_issue(SosfsFsckIssue::MirrorMismatch);
                report.status = if strict {
                    SosfsFsckStatus::Corrupt
                } else {
                    SosfsFsckStatus::Warn
                };
            }

            report
        }
        (Ok(_), Err(e)) => {
            let mut report = SosfsFsckReport::clean();
            report = report.with_issue(e);
            report.mirror_valid = [true, false];
            report.status = if strict {
                SosfsFsckStatus::Corrupt
            } else {
                SosfsFsckStatus::Warn
            };
            if let Ok(i) = info0 {
                report.info = Some(i);
            }
            report
        }
        (Err(e), Ok(_)) => {
            let mut report = SosfsFsckReport::clean();
            report = report.with_issue(e);
            report.mirror_valid = [false, true];
            report.status = if strict {
                SosfsFsckStatus::Corrupt
            } else {
                SosfsFsckStatus::Warn
            };
            if let Ok(i) = info1 {
                report.info = Some(i);
            }
            report
        }
        (Err(e0), Err(e1)) => {
            let mut report = SosfsFsckReport::clean();
            report = report.with_issue(e0).with_issue(e1);
            report.status = SosfsFsckStatus::Corrupt;
            report.mirror_valid = [false, false];
            report
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "crypto")]
    fn test_default_passkey_matches_spec() {
        assert_eq!(derive_default_passkey(), SOSFS_DEFAULT_PASSKEY);
    }

    #[test]
    fn test_superblock_probe_accepts_valid_sosfs() {
        let block = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            9,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );

        let info = probe_sosfs_superblock(&block).unwrap();
        assert_eq!(info.version_major, 0);
        assert_eq!(info.version_minor, 1);
        assert_eq!(info.active_generation, 9);
    }

    #[test]
    fn test_superblock_probe_rejects_invalid_checksum() {
        let mut block = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            1,
            2,
            128,
            130,
            64,
            194,
            1024,
            3,
        );
        block[33] ^= 0xAA;
        assert!(probe_sosfs_superblock(&block).is_none());
    }

    #[test]
    fn test_fsck_valid_mirror_pair() {
        let sb = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            9,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );

        let report = fsck_superblock_pair(&sb, &sb, false);
        assert_eq!(report.status, SosfsFsckStatus::Clean);
        assert!(report.mirror_valid[0]);
        assert!(report.mirror_valid[1]);
    }

    #[test]
    fn test_fsck_one_invalid_mirror_one_valid() {
        let valid_sb = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            9,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );

        let mut invalid_sb = valid_sb;
        invalid_sb[0] = 0xFF;

        let report = fsck_superblock_pair(&valid_sb, &invalid_sb, false);
        assert_eq!(report.status, SosfsFsckStatus::Warn);
        assert!(report.mirror_valid[0]);
        assert!(!report.mirror_valid[1]);

        let report_strict = fsck_superblock_pair(&valid_sb, &invalid_sb, true);
        assert_eq!(report_strict.status, SosfsFsckStatus::Corrupt);
    }

    #[test]
    fn test_fsck_both_invalid_mirrors() {
        let mut sb1 = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            9,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );
        let mut sb2 = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [3u8; 16],
            [4u8; 32],
            9,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );
        sb1[0] = 0xFF;
        sb2[0] = 0xFE;

        let report = fsck_superblock_pair(&sb1, &sb2, false);
        assert_eq!(report.status, SosfsFsckStatus::Corrupt);
        assert!(!report.mirror_valid[0]);
        assert!(!report.mirror_valid[1]);
    }

    #[test]
    fn test_fsck_generation_mismatch() {
        let sb1 = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            9,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );
        let sb2 = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            10,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );

        let report = fsck_superblock_pair(&sb1, &sb2, false);
        assert_eq!(report.status, SosfsFsckStatus::Warn);
        assert!(report
            .issues
            .iter()
            .any(|&x| x == Some(SosfsFsckIssue::GenerationMismatch)));

        let report_strict = fsck_superblock_pair(&sb1, &sb2, true);
        assert_eq!(report_strict.status, SosfsFsckStatus::Corrupt);
    }

    #[test]
    fn test_validate_superblock_bad_magic() {
        let mut sb = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            9,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );
        sb[0] = 0xFF;
        let result = validate_superblock(&sb);
        assert_eq!(result, Err(SosfsFsckIssue::BadMagic));
    }

    #[test]
    fn test_validate_superblock_bad_version() {
        let sb = build_superblock(
            1,
            0,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            9,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );
        let result = validate_superblock(&sb);
        assert_eq!(result, Err(SosfsFsckIssue::BadVersion));
    }

    #[test]
    fn test_validate_superblock_bad_checksum() {
        let mut sb = build_superblock(
            0,
            1,
            SOSFS_FLAG_ENCRYPTION_REQUIRED | SOSFS_FLAG_VERSIONING_REQUIRED,
            [1u8; 16],
            [2u8; 32],
            9,
            2,
            256,
            258,
            128,
            386,
            8192,
            44,
        );
        sb[100] ^= 0x01;
        let result = validate_superblock(&sb);
        assert_eq!(result, Err(SosfsFsckIssue::BadChecksum));
    }

    #[test]
    fn test_validate_superblock_bad_flags() {
        let sb = build_superblock(
            0, 1, 0, [1u8; 16], [2u8; 32], 9, 2, 256, 258, 128, 386, 8192, 44,
        );
        let result = validate_superblock(&sb);
        assert_eq!(result, Err(SosfsFsckIssue::BadFlags));
    }
}
