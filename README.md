# S.O.S. (Streamed-Object Operating System)

S.O.S. is a Rust-first framekernel prototype focused on bare-metal, low-overhead
data-plane primitives: allocator architecture, synchronization, network stack
integration, and optional TLS 1.3 transport wiring.

## Current status

- Phase 1 foundations are in place: buddy/slab allocators, spin-based sync,
  and OSTD bootstrap surface.
- Phase 2 network stack integration is implemented: VirtIO facade, smoltcp
  device integration with slab-backed DMA paths, TCP window/RTT tuning, and
  embedded-tls integration points.
- Phase 3 cryptography and storage foundations are implemented: HKDF path-key
  derivation, ChaCha20-Poly1305 AEAD, persistent WAL encode/commit/replay,
  recovery flow, copy-on-write object index, and copy-on-write fixed-fanout
  B-Tree index surface.
- Phase 4 hardening and verification has started: atomic transaction manager
  integration, lock-free CAS epoch publication path, and memory-ordering
  visibility tests for Acquire/Release semantics.
  It now also includes high-load stress tests and epoch monotonicity checks.
- Phase 5 native filesystem and tooling: sosfs format, mkfs-sosfs formatter,
  fsck-sosfs checker with strict mode, boot-time fsck gate with hard-fail halt.
- Phase 7 post-boot readiness checks are implemented as a deterministic readiness
  suite and CLI (`sos-readiness`) covering ICMP, DNS, and HTTPS probe gates.
- Phase 8 `sos-pf` is implemented with atomic nft script generation, dry-run
  kernel preflight (`sos-pf check`), apply path (`sos-pf apply`), and running
  kernel ruleset export (`sos-pf export-running`).

## Implemented modules

- `src/allocator/buddy.rs`: power-of-two allocator with split/merge coalescing.
- `src/allocator/slab.rs`: fixed-size lock-free slot allocator.
- `src/sync.rs`: interrupt-aware spinlock and `Mutex<T>` wrapper.
- `src/framekernel.rs`: OSTD allocator bootstrap and global allocator wiring.
- `src/network/virtio.rs`: VirtIO network driver facade and DMA slot lifecycle.
- `src/network/stack.rs`: smoltcp integration and socket/runtime tuning APIs.
- `src/network/tls.rs`: optional TLS 1.3 handler over `embedded-tls`.
- `src/network/readiness.rs`: phase-7 readiness suite and readiness status model.
- `src/fs/sosfs.rs`: sosfs superblock format helpers and partition probe logic.
- `src/pf/mod.rs`: phase-8 packet filter engine: YAML parser/validator,
  nft-atomic script renderer, dry-run checker, apply path, and running-ruleset
  JSON-to-YAML export bridge.
- `src/console/mod.rs`: interactive serial console, command parser,
  message-oriented console/program/pf service boundaries, and `sos-pf` program dispatch.
- `src/crypto/mod.rs`: HKDF-SHA256 path-key derivation and ChaCha20-Poly1305
  authenticated encryption helpers.
- `src/storage/mod.rs`: WAL block-device layer, crash recovery replay, COW
  object index, COW B-Tree index primitives, and atomic transaction manager.
- `src/bin/main.rs`: boot stub, serial diagnostics, and idle loop.
- `src/bin/mkfs_sosfs.rs`: `mkfs.sosfs` formatter CLI for disk images.
- `src/bin/fsck_sosfs.rs`: `fsck.sosfs` checker CLI with strict mode.
- `src/bin/sos_readiness.rs`: `sos-readiness` CLI for readiness checks.
- `src/bin/sos_pf.rs`: `sos-pf` CLI for check/apply/export/export-running operations.

## Runtime architecture

```mermaid
flowchart TD
    Boot[_start / kernel_main] --> OSTD[OSTD Framekernel Core]
    OSTD --> Buddy[BuddyAllocator]
    OSTD --> Slab[SlabAllocator]
    Slab --> DMA[DMA Frame Buffers]
    DMA --> Virtio[VirtioNetDriver]
    Virtio --> Smol[smoltcp Interface]
    Smol --> Net[NetworkStack]
    Net --> TLS[TlsHandler tls13 feature]
```

## Network/TLS flow

```mermaid
sequenceDiagram
    participant NIC as VirtioNetDriver
    participant PHY as smoltcp Device
    participant TCP as NetworkStack
    participant TLS as TlsHandler

    NIC->>PHY: receive_dma_slot()
    PHY->>TCP: RxToken.consume(frame)
    TCP->>TLS: NetworkStackIo::read/write
    TLS->>TLS: TLS 1.3 handshake (embedded-tls)
    TLS-->>TCP: Connected / Error state
    TCP->>PHY: TxToken.consume(len)
    PHY->>NIC: submit_tx_dma(ptr,len)
```

## Phase 3 storage/crypto flow

```mermaid
sequenceDiagram
    participant API as Object API
    participant Crypto as PathCrypto
    participant WAL as WriteAheadLog
    participant IDX as CowObjectIndex
    participant BT as CowBTreeIndex

    API->>Crypto: derive_object_key(path)
    API->>Crypto: encrypt_in_place(payload)
    API->>WAL: append(Put/Delete)
    API->>WAL: commit(slot)
    WAL->>IDX: replay_committed(apply_wal)
    IDX->>BT: upsert/delete key->lba
```

## Test and lint

- Unit tests: `cargo test -q`
- Unit tests (phase 7/8 included): `cargo test -q --features std`
- TLS-enabled tests: `cargo test -q --features tls13`
- Lints: `cargo clippy --all-targets --features std -- -D warnings`
- Phase 4 soak stress (release): `./scripts/phase4-stress.sh 100`
- Make target for soak stress: `make phase4-stress ITER=100`
- Phase 5 focused coverage: `make phase5-cov`

## Phase 4 atomic/CAS flow

```mermaid
flowchart LR
    Begin[Tx begin] --> WALAppend[WAL append]
    WALAppend --> WALCommit[WAL commit]
    WALCommit --> CoWMap[CoW object index apply]
    CoWMap --> CoWBTree[CoW B-Tree apply]
    CoWBTree --> CAS[CAS bump epoch AcqRel]
    CAS --> Publish[Readers observe epoch Acquire]
```

For deeper design notes and module-level diagrams, see `ARCHITECTURE.md`.

## sosfs formatter quickstart

```bash
# Format a new filesystem image
cargo run --features "std,crypto" --bin mkfs-sosfs -- --image sosfs.img --blocks 32768
```

## sosfs checker quickstart

```bash
# Check a filesystem image (non-strict mode)
cargo run --features std --bin fsck-sosfs -- --image sosfs.img

# Check in strict mode (any issue results in corrupt status)
cargo run --features std --bin fsck-sosfs -- --image sosfs.img --strict
```

Exit codes:
- 0 = clean
- 1 = warn (non-strict only)
- 2 = corrupt
- 3 = io/usage error

Formatter notes are also in `tools/sosfs-mkfs/README.md`.

## `sos-readiness` quickstart

```bash
# Run readiness checks (icmp/dns/https)
cargo run --features std --bin sos-readiness -- --strict
```

## `sos-pf` quickstart

```bash
# Validate a packet filter YAML config
cargo run --features std --bin sos-pf -- check --config packet-filter.yaml

# Apply config atomically through nft batch file mode
cargo run --features std --bin sos-pf -- apply --config packet-filter.yaml

# Export canonicalized YAML from a config
cargo run --features std --bin sos-pf -- export --config packet-filter.yaml

# Export running kernel nftables ruleset as sos-pf YAML
cargo run --features std --bin sos-pf -- export-running
```

## Boot console behavior

- On bare-metal target boot (`target_os = "none"`), after sosfs probe and fsck,
  S.O.S. starts an interactive serial console automatically.
- Prompt: `sos> `
- Builtin console commands:
  - `help` (shows console usage)
  - `help <program>` (shows ABI/version/summary)
  - `programs` (lists registered OS programs)
  - `history` (prints bounded history ring)
- First available OS program: `sos-pf`
  - `sos-pf check`
  - `sos-pf apply`
  - `sos-pf status`
  - `sos-pf export`
  - `sos-pf export-running`
- `sos-pf apply` updates kernel runtime PF state generation.
- `sos-pf status` reports exact runtime state + exact generation value.
- `sos-pf export`/`export-running` include runtime metadata plus active policy.
- Program service exposes spawn/wait/terminate lifecycle contracts internally,
  keeping console parsing, dispatch, and privileged control actions separated.
- Boot emits deterministic self-check and timing transcript before prompt.
- Isolation/runtime foundation now includes:
  - process-slot allocator with per-process address-space regions
  - executable header parser (`SOSX` magic + ABI metadata)
  - VM context objects (`CpuContext`) and VM ops trait (`VmContextOps`)
  - context slot installation and runtime context switching hooks
  - IPC bus with endpoint registration and bounded queued message passing
