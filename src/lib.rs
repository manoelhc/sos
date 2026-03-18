//! S.O.S. crate root.
//!
//! This crate exposes the framekernel-oriented building blocks used by the
//! project:
//! - memory allocators (`BuddyAllocator`, `SlabAllocator`),
//! - synchronization primitives (`Spinlock`, `Mutex`, `AtomicSlabBitmap`),
//! - a minimal OSTD runtime surface,
//! - bare-metal networking primitives (`VirtioNetDriver`, `NetworkStack`),
//! - optional TLS 1.3 integration (`tls13` feature).

#![cfg_attr(all(not(feature = "std"), not(test), target_os = "none"), no_std)]

#[cfg(any(feature = "std", test, not(target_os = "none")))]
extern crate std;

pub mod allocator;
pub mod console;
#[cfg(feature = "crypto")]
pub mod crypto;
pub mod framekernel;
pub mod fs;
pub mod network;
#[cfg(feature = "std")]
pub mod pf;
pub mod process;
pub mod storage;
pub mod sync;

pub use allocator::{BuddyAllocator, SlabAllocator};
pub use console::{
    parse_command_line, BootSelfCheckReport, ConsoleError, ConsoleReader, ConsoleService,
    ConsoleWriter, KernelPacketFilterControl, MachineErrorCode, MonotonicClock,
    PacketFilterControl, PfMessage, PfResult, PfService, PfServiceImpl, Program, ProgramAbi,
    ProgramDescriptor, ProgramHandle, ProgramRegistry, ProgramRequest, ProgramResponse,
    ProgramService, ProgramServiceImpl, ProgramState, SosPfProgram, BOOT_PROMPT_BUDGET_MS,
};
#[cfg(feature = "crypto")]
pub use crypto::{PathCrypto, DERIVED_KEY_SIZE, NONCE_SIZE, TAG_SIZE};
pub use framekernel::OSTD;
pub use fs::{
    fsck_superblock_pair, probe_sosfs_superblock, validate_superblock, SosfsFsckIssue,
    SosfsFsckReport, SosfsFsckStatus, SosfsSuperblockInfo, SOSFS_BLOCK_SIZE, SOSFS_MAGIC,
};
pub use network::{
    default_client_config, NetworkResources, NetworkStack, TcpSocketConfig, TcpWindowScaler,
    TlsHandler, TlsState, VirtioNetDriver, VirtioRxToken, VirtioTxToken, TLS_MAX_FRAME_SIZE,
};
#[cfg(feature = "tls13")]
pub use network::{NetworkIoError, NetworkStackIo};
#[cfg(feature = "std")]
pub use network::{ReadinessCheck, ReadinessStatus, ReadinessSuite};
#[cfg(feature = "std")]
pub use pf::{
    apply_with_runner as pf_apply_with_runner, build_apply_plan as pf_build_apply_plan,
    check_config as pf_check_config, dry_run_check_with_runner as pf_dry_run_check_with_runner,
    export_config_yaml as pf_export_config_yaml,
    export_running_ruleset_yaml_with_runner as pf_export_running_ruleset_yaml_with_runner,
    parse_config as pf_parse_config, ruleset_json_to_yaml as pf_ruleset_json_to_yaml, ApplyPlan,
    NftRunner, PfCheckReport, PfConfig, PfError, SystemNftRunner,
};
pub use process::{
    parse_executable_header, AbiVersion, AddressSpace, CpuContext, ExecutableHeader, IpcBus,
    IpcEndpoint, IpcMessage, IsolationError, IsolationRuntime,
    ProcessHandle as IsolatedProcessHandle, ProcessState as IsolatedProcessState, VmContextOps,
    CONTEXT_SLOT_CAPACITY, IPC_QUEUE_CAPACITY, PROCESS_SLOT_CAPACITY,
};
pub use storage::{
    AtomicTransactionManager, BTreeNode, BTreeNodeEntry, CowBTreeIndex, CowObjectIndex,
    InMemoryWalDevice, ObjectEntry, TxError, TxStatus, WalBlockDevice, WalOp, WalRecord,
    WriteAheadLog, WAL_BLOCK_SIZE,
};
pub use sync::{AtomicSlabBitmap, Mutex, Spinlock};

#[cfg(all(feature = "lib-panic", not(feature = "std"), target_os = "none"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::alloc::Layout;
    use core::mem::MaybeUninit;

    #[test]
    fn test_buddy_allocator() {
        let heap: MaybeUninit<[u8; BuddyAllocator::HEAP_SIZE]> = MaybeUninit::uninit();
        let allocator =
            unsafe { BuddyAllocator::new(heap.as_ptr() as usize, BuddyAllocator::HEAP_SIZE) };
        let layout = Layout::from_size_align(1024, 8).unwrap();
        let ptr = unsafe { allocator.alloc(layout) };
        assert!(!ptr.is_null());
        unsafe { allocator.dealloc(ptr, layout) };
    }

    #[test]
    fn test_slab_allocator() {
        let mut allocator = SlabAllocator::new(64, 16);
        unsafe { allocator.init(0x1000) };
        let ptr = unsafe { allocator.alloc() };
        assert!(!ptr.is_null());
        unsafe { allocator.dealloc(ptr) };
    }

    #[test]
    fn test_spinlock() {
        let lock = Spinlock::new();
        assert!(!lock.is_locked());
        lock.lock();
        assert!(lock.is_locked());
        lock.unlock();
        assert!(!lock.is_locked());
    }

    #[test]
    fn test_mutex() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock();
        assert_eq!(*guard, 42);
    }
}
