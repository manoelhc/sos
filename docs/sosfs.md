# S.O.S. Filesystem (sosfs) v0 Draft

This document defines the initial on-disk format for the S.O.S. native
filesystem (`sosfs`) to satisfy Phase 5 requirements:

- partition-recognizable custom filesystem format,
- object versioning,
- encryption by default,
- deterministic formatter/checker tooling.

## Goals

- Use a simple append-friendly layout suitable for raw block devices.
- Make all stored object payloads encrypted/authenticated by default.
- Preserve object history through immutable versions.
- Allow deterministic recovery from WAL + index rebuild.

## Cryptographic defaults

- Cipher: ChaCha20-Poly1305.
- Key derivation: HKDF-SHA256.
- Default passkey: `sha256("sos")` (32 bytes).
- Per-object key material:
  - `object_key = HKDF(passkey, fs_salt, object_path_hash)`
  - `object_path_hash = sha256(path)`
- Nonce size: 96 bits.
- Authentication tag size: 128 bits.

## Block and partition assumptions

- Logical block size: 4096 bytes (required by format v0).
- Partition must expose contiguous LBAs.
- Endianness: little-endian for all integral fields.

## High-level layout

```text
LBA 0             : Superblock A
LBA 1             : Superblock B (mirror)
LBA 2..(2+N-1)    : Journal/WAL region
LBA ...           : Object metadata/index region
LBA ...           : Object payload region (versioned immutable chunks)
```

Rationale:
- mirrored superblocks allow atomic generation switch,
- append-only WAL preserves atomicity and durability,
- object payloads remain immutable and version-addressed.

## Superblock

Superblock size is one block (4096 bytes).

### Header fields

- `magic[8]`: `"SOSFS\0\0\0"` (canonical 8-byte magic).
- `version_major: u16`.
- `version_minor: u16`.
- `block_size: u32` (must be 4096).
- `flags: u64`.
- `fs_uuid: [u8; 16]`.
- `fs_salt: [u8; 32]`.
- `active_generation: u64`.
- `wal_start_lba: u64`.
- `wal_blocks: u64`.
- `index_start_lba: u64`.
- `index_blocks: u64`.
- `data_start_lba: u64`.
- `data_blocks: u64`.
- `last_checkpoint_wal_seq: u64`.
- `checksum32: u32` (of superblock bytes excluding checksum field).

### Flags

- `0x1`: encryption required (must be set in v0).
- `0x2`: versioning required (must be set in v0).
- `0x4`: clean unmount marker.

## Object model

Objects are addressed by path hash:

- `object_id = sha256(path_utf8_bytes)`

Each write creates a new immutable version record:

- `version_id: u64` monotonically increasing per object.
- Metadata points latest version for fast reads.
- Older versions are retained until GC policy reclaims them.

## WAL records

WAL is append-only fixed-size records (current implementation aligns to 64-byte
encoded entries; on-disk may pack multiple records per block).

### Record types

- `PUT_VERSION`
  - object_id
  - version_id
  - payload_lba
  - payload_len
  - nonce
  - tag
  - metadata hash
- `DELETE_OBJECT`
  - object_id
- `CHECKPOINT`
  - sequence, index snapshot pointers

Each record includes:

- `record_magic`
- `record_version`
- `tx_id`
- `committed` marker
- `checksum32`

Commit rule:
- A transaction is durable only after WAL record is persisted and commit marker
  is visible.

## Index region

v0 index combines:

- CoW object map snapshots for fast key->latest-version lookup.
- CoW B-Tree nodes for ordered traversal and deterministic rebuild.

Primary key: `object_id`.
Value:

- latest `version_id`
- version-chain head pointer (or compact list root)
- logical metadata (timestamps/flags)

## Payload format

Each payload extent stores encrypted bytes only:

- ciphertext bytes
- associated nonce/tag tracked in metadata/WAL

AAD recommendation:

- `object_id || version_id || payload_len`

## Mount and recognition flow

1. Read superblocks A and B.
2. Validate `magic`, `version`, `block_size`, and checksums.
3. Choose highest valid `active_generation`.
4. Verify required flags (`encryption`, `versioning`).
5. Replay WAL from last checkpoint to rebuild in-memory indices.
6. Mark filesystem mounted (clear clean bit until orderly unmount).

## Formatter tool requirements (`mkfs.sosfs`)

The external formatter must:

- initialize both superblocks,
- generate `fs_uuid` and `fs_salt`,
- set encryption/versioning required flags,
- predefine WAL/index/data regions,
- derive default master passkey from `sha256("sos")` if no explicit key is
  provided,
- emit machine-readable metadata report (JSON).

## Checker tool requirements (`fsck.sosfs`)

Checker must:

- validate superblock mirror consistency,
- scan WAL for checksum/ordering errors,
- verify index references to valid data extents,
- detect missing/corrupt version chains,
- optionally rebuild index from WAL.

### fsck rules and exit codes

The `fsck-sosfs` tool validates superblock mirrors and reports filesystem health:

| Status | Exit Code (non-strict) | Exit Code (strict) | Description |
|--------|------------------------|-------------------|-------------|
| Clean  | 0                      | 0                 | Both mirrors valid and consistent |
| Warn   | 1                      | 2                 | One mirror invalid or divergence |
| Corrupt| 2                      | 2                 | Both mirrors invalid |
| Error  | 3                      | 3                 | I/O or usage error |

### strict mode semantics

- **One invalid mirror**: Non-strict = Warn, Strict = Corrupt
- **Generation mismatch**: Non-strict = Warn, Strict = Corrupt  
- **Mirror divergence (UUID/salt/flags mismatch)**: Non-strict = Warn, Strict = Corrupt

### Boot hard-fail gate

On boot, after partition recognition:

1. Read superblock mirrors (LBA 0 and LBA 1)
2. Run `fsck_superblock_pair(sb0, sb1, strict=true)`
3. If status is Corrupt:
   - Print diagnostic to serial: `[sos] fsck: corrupt`
   - Print reason for each issue
   - Halt with: `[sos] fsck: HALT`
4. If status is Clean or Warn (non-strict):
   - Continue boot normally

Serial log examples:
- `[sos] fsck: clean` - filesystem OK
- `[sos] fsck: corrupt` / `[sos] fsck: reason=bad_magic` / `[sos] fsck: HALT` - hard failure

## Compatibility and versioning policy

- v0 is experimental and may change on incompatible boundaries.
- Any incompatible change must bump `version_major`.
- Tooling must refuse mount/format mismatch unless `--force` is provided.

## Open items for implementation

- Garbage collection policy for stale versions.
- Optional passkey rotation protocol.
- Compression pipeline compatibility.
- Formalized binary structs and offsets for all record types.
