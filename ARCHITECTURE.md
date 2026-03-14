# Architecture Notes

This document maps the current repository implementation to the S.O.S.
framekernel roadmap, including completed Phase 2 networking and Phase 3
cryptography/storage foundations.

## Module map

```mermaid
flowchart TD
    LIB[lib.rs] --> ALLOC[allocator]
    LIB --> SYNC[sync]
    LIB --> FK[framekernel]
    LIB --> NET[network]
    LIB --> CRYPTO[crypto]
    LIB --> STORAGE[storage]
    LIB --> FS[fs]

    ALLOC --> BUDDY[buddy.rs]
    ALLOC --> SLAB[slab.rs]
    SLAB --> SYNC

    NET --> VIRTIO[virtio.rs]
    NET --> STACK[stack.rs]
    NET --> TLS[tls.rs]

    VIRTIO --> STACK
    STACK --> TLS
    CRYPTO --> STORAGE
    FS --> STORAGE
```

## Boot and framekernel runtime

- `_start` in `src/bin/main.rs` sets a known stack pointer and enters
  `kernel_main`.
- `kernel_main` initializes COM1 and emits boot diagnostics.
- The kernel then idles in an `hlt` loop.
- `OSTD` in `src/framekernel.rs` provides the core allocator-backed runtime
  surface and singleton allocator bootstrap.

```mermaid
sequenceDiagram
    participant CPU
    participant Entry as _start
    participant Kernel as kernel_main
    participant OSTD
    participant UART as COM1

    CPU->>Entry: jump to entry point
    Entry->>Entry: set RSP/RBP
    Entry->>Kernel: call kernel_main
    Kernel->>UART: serial_init + boot log
    Kernel->>OSTD: allocator-backed services available
    Kernel->>CPU: hlt idle loop
```

## Memory subsystem

### Buddy allocator (`src/allocator/buddy.rs`)

- Manages power-of-two blocks using order-based intrusive free lists.
- Allocation: locate first available order, split downward.
- Deallocation: insert and recursively merge buddy blocks.

```mermaid
flowchart TD
    Req[alloc(layout)] --> Ord[compute required order]
    Ord --> Scan{free list has block?}
    Scan -- no --> Up[next higher order]
    Up --> Scan
    Scan -- yes --> Pop[pop block]
    Pop --> Split{order > required?}
    Split -- yes --> PushBuddy[split + push buddy]
    PushBuddy --> Split
    Split -- no --> Ret[return ptr]
```

### Slab allocator (`src/allocator/slab.rs`)

- Fixed-size object slots over a pre-provisioned memory region.
- Uses atomic bitmap bit operations for fast claim/release.
- Forms the core buffer source for networking DMA slots.

## Synchronization model (`src/sync.rs`)

- `Spinlock` disables local interrupts before CAS lock attempts.
- Failed lock attempts briefly restore interrupt state before retry.
- `Mutex<T>` builds scoped guarded access atop `Spinlock`.
- `AtomicSlabBitmap` offers lock-free slot tracking.

```mermaid
stateDiagram-v2
    [*] --> Unlocked
    Unlocked --> AcquireIRQMask: lock()
    AcquireIRQMask --> TryCAS
    TryCAS --> Locked: success
    TryCAS --> SpinRetry: fail
    SpinRetry --> AcquireIRQMask
    Locked --> RestoreIRQMask: unlock()
    RestoreIRQMask --> Unlocked
```

## Phase 2 networking stack

### VirtIO facade (`src/network/virtio.rs`)

- Exposes MMIO init/status/feature negotiation hooks.
- Maintains slab-backed frame slot ring.
- Provides explicit DMA lifecycle:
  - `alloc_dma_buffer`
  - `submit_tx_dma`
  - `receive_dma_slot`
  - `release_dma_buffer`

### smoltcp integration (`src/network/stack.rs`)

- Implements `smoltcp::phy::Device` for `VirtioNetDriver`.
- RX token reads directly from DMA memory and releases slot after consume.
- TX token writes packet bytes directly into slab DMA buffers.
- Configures TCP behavior (nagle/keepalive/ack-delay/timeout profile).

### TCP window scaling and RTT calibration (`src/network/stack.rs`)

- `TcpWindowScaler::recommended_window_bytes` computes BDP-inspired target.
- `calibrate_buffers` derives rx/tx sizes and effective scaling.
- `apply_rtt_profile` applies dynamic config based on RTT.

### TLS 1.3 integration (`src/network/tls.rs` + `src/network/stack.rs`)

- `TlsHandler` wraps `embedded-tls` blocking API with state machine tracking.
- `NetworkStackIo` adapts the TCP stack to `embedded-io` Read/Write traits.
- Integration tests cover:
  - failure-path wiring on isolated network stack,
  - successful handshake with local mocked rustls server transcript.

```mermaid
sequenceDiagram
    participant App as Caller
    participant Stack as NetworkStack
    participant PHY as smoltcp Device
    participant Virt as VirtioNetDriver
    participant TLS as TlsHandler

    Virt->>PHY: receive_dma_slot
    PHY->>Stack: RxToken.consume(&[u8])
    App->>Stack: tls_io()
    App->>TLS: open(NetworkStackIo,...)
    TLS->>Stack: read/write via embedded-io
    Stack->>PHY: TxToken.consume
    PHY->>Virt: submit_tx_dma
```

## Validation status

- `cargo test -q` passing.
- `cargo test -q --features tls13` passing.
- `cargo clippy` passing.

## Phase 3 cryptography and storage

### Crypto module (`src/crypto/mod.rs`)

- HKDF-SHA256 based path-key derivation:
  - `PathCrypto::path_hash`
  - `PathCrypto::derive_object_key`
- ChaCha20-Poly1305 AEAD helpers:
  - `encrypt_in_place`
  - `decrypt_in_place`

### WAL module (`src/storage/mod.rs`)

- Encodes WAL records into fixed-size blocks with magic/version/checksum.
- Persists records through a `WalBlockDevice` trait.
- Supports append, commit, replay, and tail/head recovery after corruption.

### CoW indices (`src/storage/mod.rs`)

- `CowObjectIndex` publishes map snapshots by atomic active-slot switch.
- `CowBTreeIndex` publishes B-Tree root snapshots by atomic active-slot switch.
- B-Tree entries are fixed-size and kept sorted for deterministic lookup.

```mermaid
flowchart TD
    ClientWrite[Object write request] --> Derive[HKDF derive key from path]
    Derive --> Encrypt[ChaCha20-Poly1305 encrypt]
    Encrypt --> Append[WAL append record]
    Append --> Commit[WAL commit]
    Commit --> Replay[Recovery replay path]
    Replay --> ObjIdx[CowObjectIndex apply_wal]
    ObjIdx --> BTree[CowBTreeIndex upsert/delete]
```

### Phase 3 integration tests

- Crypto key-binding and AEAD roundtrip tests (`src/crypto/mod.rs`).
- WAL commit/replay and recovery corruption tests (`src/storage/mod.rs`).
- CoW object index and CoW B-Tree publication tests (`src/storage/mod.rs`).
- End-to-end Phase 3 flow test:
  encrypt -> WAL append/commit -> replay -> index apply -> decrypt verify
  (`src/storage/mod.rs`).

## Phase 4 hardening and verification (in progress)

### Atomic transaction manager (`src/storage/mod.rs`)

- `AtomicTransactionManager` integrates transaction state transitions:
  - `InFlight` -> `Committed`/`Aborted`
  - WAL append/commit ordering before index publication
- CoW object + B-Tree publication occurs before epoch CAS bump.

### CAS + memory ordering strategy

- Epoch publication uses CAS (`compare_exchange_weak`) with `Ordering::AcqRel`.
- Readers use `Ordering::Acquire` on epoch/status reads.
- Writer side uses `Ordering::Release` status publication.

```mermaid
sequenceDiagram
    participant W as Writer
    participant T as TxManager
    participant R as Reader

    W->>T: put/delete request
    T->>T: WAL append + commit
    T->>T: apply CoW object/B-Tree
    T->>T: CAS epoch (AcqRel)
    R->>T: load epoch (Acquire)
    R->>R: safely observe published state
```

### Phase 4 verification tests added

- Atomic transaction put/delete commit path.
- Recovery replay rebuilding object/B-Tree indices.
- Acquire/Release visibility litmus test validating publication ordering.
- High-load stress test validating transaction durability and replay equivalence
  across thousands of operations.
- Epoch monotonicity test ensuring CAS-based publication progresses safely under
  repeated updates.
- Soak runner script for repeated release-mode execution:
  `scripts/phase4-stress.sh`.

## Phase 5 native filesystem and tooling (implemented)

- `sosfs` on-disk specification in `docs/sosfs.md`.
- Canonical 8-byte magic: `"SOSFS\0\0\0"`.
- Formatter CLI: `mkfs-sosfs` (`src/bin/mkfs_sosfs.rs`).
- Checker CLI: `fsck-sosfs` (`src/bin/fsck_sosfs.rs`).
- fsck core library in `src/fs/sosfs.rs`:
  - `validate_superblock()` - validate single superblock
  - `fsck_superblock_pair()` - validate mirror pair with strict mode
- Boot integration: fsck runs after partition recognition, halts on corruption.
- Serial logs: `[sos] fsck: clean`, `[sos] fsck: corrupt`, `[sos] fsck: HALT`

```mermaid
flowchart TD
    MKFS[mkfs-sosfs] --> IMG[sosfs image]
    IMG --> Boot[kernel_main]
    Boot --> Probe[probe_sosfs_superblock]
    Probe --> Detected{valid magic checksum flags?}
    Detected -- yes --> Fsck[fsck_superblock_pair]
    Detected -- no --> Fallback[partition rejected]
    Fsck --> Clean{status clean?}
    Clean -- yes --> Ready[sosfs usable]
    Clean -- no --> Halt[HALT on corrupt]
```
