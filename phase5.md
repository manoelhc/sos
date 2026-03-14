Phase 5 Implementation Plan (to be added to docs)
1. sosfs magic fix
   - Canonical 8-byte magic: "SOSFS\0\0\0" (hex: 53 4f 53 46 53 00 00 00)
   - Align code and docs to this exact specification.
2. Shared fsck core (library, no-std compatible)
   - Add types:
     - SosfsFsckStatus (Clean, Warn, Corrupt)
     - SosfsFsckIssue (bad magic/version/checksum/flags/block-size/mirror mismatch)
     - SosfsFsckReport
   - Add functions:
     - validate_superblock(block) -> Result<Info, Issue>
     - fsck_superblock_pair(sb0, sb1, strict) -> Report
   - Strict mode:
     - One invalid mirror => Corrupt
     - Mirror divergence (including generation mismatch) => Corrupt
3. External checker binary fsck-sosfs
   - New binary: src/bin/fsck_sosfs.rs
   - CLI options: --image <path>, --strict
   - Exit codes:
     - 0 = clean
     - 1 = warn (non-strict only)
     - 2 = corrupt
     - 3 = io/usage error
   - Register in Cargo.toml with required-features = ["std"]
4. Boot integration (hard-fail policy)
   - After partition recognition + mount in src/bin/main.rs:
     - Run fsck on superblock mirrors
     - If status is Corrupt: print diagnostic + panic/halt
   - Serial logs:
     - [sos] fsck: clean
     - [sos] fsck: corrupt (reason=...)
     - [sos] fsck: HALT
5. Tests
   - Unit tests in src/fs/sosfs.rs:
     - Valid mirror pair
     - One invalid mirror, one valid
     - Both invalid mirrors
     - Generation mismatch
     - Strict vs non-strict behavior differences
   - CLI exit-code tests
6. Coverage push
   - Focus cargo llvm-cov on:
     - src/fs/*
     - src/bin/mkfs_sosfs.rs
     - src/bin/fsck_sosfs.rs
   - Target: near 100% lines/regions for Phase 5 scope
7. Documentation updates
   - docs/sosfs.md:
     - Align magic bytes (8-byte canonical)
     - Document fsck rules, strict mode semantics, boot hard-fail gate
   - README.md:
     - Add mkfs + fsck quickstart examples
     - Include --strict usage
   - ARCHITECTURE.md:
     - Mark Phase 5 as implemented
     - Update boot flow diagram to include fsck gate