# mkfs.sosfs tool

The formatter binary is provided by the main workspace as:

- `mkfs-sosfs` (`src/bin/mkfs_sosfs.rs`, requires `--features std`)

Example:

```bash
cargo run --features std --bin mkfs-sosfs -- --image sosfs.img --blocks 32768
```

This initializes:

- mirrored superblocks at LBA 0 and LBA 1,
- WAL/index/data layout,
- encryption/versioning required flags,
- default passkey derivation from `sha256("sos")`.
