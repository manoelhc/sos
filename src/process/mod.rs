//! Process isolation, VM contexts, scheduler, and IPC runtime.

use crate::sync::Mutex;

pub const PROCESS_SLOT_CAPACITY: usize = 16;
pub const CONTEXT_SLOT_CAPACITY: usize = 16;
pub const IPC_QUEUE_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiVersion {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessHandle {
    pub pid: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Spawned,
    Ready,
    Running,
    Blocked,
    Exited(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressSpace {
    pub asid: u32,
    pub code_base: u64,
    pub data_base: u64,
    pub stack_base: u64,
    pub span_bytes: u64,
    pub pml4_phys: u64,
}

impl AddressSpace {
    pub fn overlaps(&self, other: &AddressSpace) -> bool {
        let self_end = self.code_base.saturating_add(self.span_bytes);
        let other_end = other.code_base.saturating_add(other.span_bytes);
        self.code_base < other_end && other.code_base < self_end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsolationError {
    NoSlots,
    UnknownProcess,
    BadExecutable,
    BadState,
    BadContext,
    QueueFull,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutableHeader {
    pub abi: AbiVersion,
    pub entry_offset: u32,
    pub image_size: u32,
}

const EXEC_MAGIC: [u8; 4] = *b"SOSX";

pub fn parse_executable_header(bytes: &[u8]) -> Result<ExecutableHeader, IsolationError> {
    if bytes.len() < 16 {
        return Err(IsolationError::BadExecutable);
    }
    if bytes[0..4] != EXEC_MAGIC {
        return Err(IsolationError::BadExecutable);
    }

    let abi = AbiVersion {
        major: u16::from_le_bytes([bytes[4], bytes[5]]),
        minor: u16::from_le_bytes([bytes[6], bytes[7]]),
    };
    let entry_offset = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let image_size = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    if image_size == 0 || entry_offset >= image_size {
        return Err(IsolationError::BadExecutable);
    }

    Ok(ExecutableHeader {
        abi,
        entry_offset,
        image_size,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuContext {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub cr3: u64,
}

impl CpuContext {
    pub const fn user(entry: u64, stack_top: u64, cr3: u64) -> Self {
        Self {
            rip: entry,
            rsp: stack_top,
            rflags: 0x202,
            cr3,
        }
    }
}

pub trait VmContextOps {
    fn map_user_region(
        &self,
        aspace: &AddressSpace,
        vaddr: u64,
        paddr: u64,
        len: u64,
    ) -> Result<(), IsolationError>;

    fn install_context(&self, ctx: &CpuContext) -> Result<(), IsolationError>;
}

#[derive(Clone, Copy)]
struct ProcessSlot {
    used: bool,
    pid: u32,
    state: ProcessState,
    aspace: AddressSpace,
    executable: Option<ExecutableHeader>,
}

impl ProcessSlot {
    const fn empty() -> Self {
        Self {
            used: false,
            pid: 0,
            state: ProcessState::Spawned,
            aspace: AddressSpace {
                asid: 0,
                code_base: 0,
                data_base: 0,
                stack_base: 0,
                span_bytes: 0,
                pml4_phys: 0,
            },
            executable: None,
        }
    }
}

#[derive(Clone, Copy)]
struct ContextSlot {
    used: bool,
    pid: u32,
    ctx: CpuContext,
}

impl ContextSlot {
    const fn empty() -> Self {
        Self {
            used: false,
            pid: 0,
            ctx: CpuContext {
                rip: 0,
                rsp: 0,
                rflags: 0,
                cr3: 0,
            },
        }
    }
}

struct RuntimeState<const P: usize, const C: usize> {
    next_pid: u32,
    next_asid: u32,
    next_pml4_phys: u64,
    processes: [ProcessSlot; P],
    contexts: [ContextSlot; C],
    current_pid: Option<u32>,
}

impl<const P: usize, const C: usize> RuntimeState<P, C> {
    const fn new() -> Self {
        Self {
            next_pid: 1,
            next_asid: 1,
            next_pml4_phys: 0x1000_0000,
            processes: [const { ProcessSlot::empty() }; P],
            contexts: [const { ContextSlot::empty() }; C],
            current_pid: None,
        }
    }
}

pub struct IsolationRuntime<const P: usize, const C: usize> {
    state: Mutex<RuntimeState<P, C>>,
}

impl<const P: usize, const C: usize> IsolationRuntime<P, C> {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(RuntimeState::new()),
        }
    }

    pub fn spawn_process(&self) -> Result<ProcessHandle, IsolationError> {
        let mut state = self.state.lock();
        let idx = state
            .processes
            .iter()
            .position(|s| !s.used)
            .ok_or(IsolationError::NoSlots)?;

        let pid = state.next_pid;
        state.next_pid = state.next_pid.saturating_add(1);
        let asid = state.next_asid;
        state.next_asid = state.next_asid.saturating_add(1);
        let pml4_phys = state.next_pml4_phys;
        state.next_pml4_phys = state.next_pml4_phys.saturating_add(0x1000);

        let base = 0x4000_0000u64 + (asid as u64) * 0x0100_0000u64;
        state.processes[idx] = ProcessSlot {
            used: true,
            pid,
            state: ProcessState::Spawned,
            aspace: AddressSpace {
                asid,
                code_base: base,
                data_base: base + 0x0020_0000,
                stack_base: base + 0x00F0_0000,
                span_bytes: 0x0100_0000,
                pml4_phys,
            },
            executable: None,
        };

        Ok(ProcessHandle { pid })
    }

    pub fn load_executable(
        &self,
        handle: ProcessHandle,
        image: &[u8],
    ) -> Result<(), IsolationError> {
        let header = parse_executable_header(image)?;
        let mut state = self.state.lock();
        let proc = state
            .processes
            .iter_mut()
            .find(|p| p.used && p.pid == handle.pid)
            .ok_or(IsolationError::UnknownProcess)?;
        proc.executable = Some(header);
        proc.state = ProcessState::Ready;
        Ok(())
    }

    pub fn build_context(&self, handle: ProcessHandle) -> Result<CpuContext, IsolationError> {
        let state = self.state.lock();
        let proc = state
            .processes
            .iter()
            .find(|p| p.used && p.pid == handle.pid)
            .ok_or(IsolationError::UnknownProcess)?;
        let exec = proc.executable.ok_or(IsolationError::BadState)?;

        let entry = proc.aspace.code_base + exec.entry_offset as u64;
        let stack_top = proc.aspace.stack_base + 0x000F_F000;
        Ok(CpuContext::user(entry, stack_top, proc.aspace.pml4_phys))
    }

    pub fn install_context_slot(
        &self,
        handle: ProcessHandle,
        ctx: CpuContext,
    ) -> Result<(), IsolationError> {
        let mut state = self.state.lock();
        if state
            .contexts
            .iter_mut()
            .any(|c| c.used && c.pid == handle.pid)
        {
            for slot in &mut state.contexts {
                if slot.used && slot.pid == handle.pid {
                    slot.ctx = ctx;
                    return Ok(());
                }
            }
            return Err(IsolationError::BadContext);
        }

        for slot in &mut state.contexts {
            if !slot.used {
                *slot = ContextSlot {
                    used: true,
                    pid: handle.pid,
                    ctx,
                };
                return Ok(());
            }
        }
        Err(IsolationError::NoSlots)
    }

    pub fn switch_to(
        &self,
        handle: ProcessHandle,
        vm: &dyn VmContextOps,
    ) -> Result<(), IsolationError> {
        let mut state = self.state.lock();

        let proc_idx = state
            .processes
            .iter()
            .position(|p| p.used && p.pid == handle.pid)
            .ok_or(IsolationError::UnknownProcess)?;

        if !matches!(
            state.processes[proc_idx].state,
            ProcessState::Ready | ProcessState::Running
        ) {
            return Err(IsolationError::BadState);
        }

        let ctx = state
            .contexts
            .iter()
            .find(|c| c.used && c.pid == handle.pid)
            .ok_or(IsolationError::BadContext)?
            .ctx;

        vm.install_context(&ctx)?;

        if let Some(prev) = state.current_pid {
            if let Some(prev_idx) = state.processes.iter().position(|p| p.used && p.pid == prev) {
                if state.processes[prev_idx].state == ProcessState::Running {
                    state.processes[prev_idx].state = ProcessState::Ready;
                }
            }
        }

        state.processes[proc_idx].state = ProcessState::Running;
        state.current_pid = Some(handle.pid);
        Ok(())
    }

    pub fn map_user_layout(
        &self,
        handle: ProcessHandle,
        vm: &dyn VmContextOps,
    ) -> Result<(), IsolationError> {
        let state = self.state.lock();
        let proc = state
            .processes
            .iter()
            .find(|p| p.used && p.pid == handle.pid)
            .ok_or(IsolationError::UnknownProcess)?;

        vm.map_user_region(
            &proc.aspace,
            proc.aspace.code_base,
            0x2000_0000,
            0x0020_0000,
        )?;
        vm.map_user_region(
            &proc.aspace,
            proc.aspace.data_base,
            0x2200_0000,
            0x00D0_0000,
        )?;
        vm.map_user_region(
            &proc.aspace,
            proc.aspace.stack_base,
            0x2F00_0000,
            0x0010_0000,
        )?;
        Ok(())
    }

    pub fn transition(
        &self,
        handle: ProcessHandle,
        new_state: ProcessState,
    ) -> Result<(), IsolationError> {
        let mut state = self.state.lock();
        for proc in &mut state.processes {
            if proc.used && proc.pid == handle.pid {
                proc.state = new_state;
                return Ok(());
            }
        }
        Err(IsolationError::UnknownProcess)
    }

    pub fn state_of(&self, handle: ProcessHandle) -> Result<ProcessState, IsolationError> {
        let state = self.state.lock();
        for proc in &state.processes {
            if proc.used && proc.pid == handle.pid {
                return Ok(proc.state);
            }
        }
        Err(IsolationError::UnknownProcess)
    }

    pub fn address_space_of(&self, handle: ProcessHandle) -> Result<AddressSpace, IsolationError> {
        let state = self.state.lock();
        for proc in &state.processes {
            if proc.used && proc.pid == handle.pid {
                return Ok(proc.aspace);
            }
        }
        Err(IsolationError::UnknownProcess)
    }

    pub fn terminate(&self, handle: ProcessHandle) -> Result<(), IsolationError> {
        let mut state = self.state.lock();

        for ctx in &mut state.contexts {
            if ctx.used && ctx.pid == handle.pid {
                *ctx = ContextSlot::empty();
            }
        }

        if let Some(idx) = state
            .processes
            .iter()
            .position(|p| p.used && p.pid == handle.pid)
        {
            if state.current_pid == Some(handle.pid) {
                state.current_pid = None;
            }
            state.processes[idx] = ProcessSlot::empty();
            return Ok(());
        }
        Err(IsolationError::UnknownProcess)
    }
}

impl<const P: usize, const C: usize> Default for IsolationRuntime<P, C> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpcEndpoint {
    pub id: u32,
    pub owner: ProcessHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpcMessage {
    pub from: IpcEndpoint,
    pub to: IpcEndpoint,
    pub len: usize,
    pub payload: [u8; 64],
}

impl IpcMessage {
    pub fn from_bytes(from: IpcEndpoint, to: IpcEndpoint, bytes: &[u8]) -> Self {
        let mut payload = [0u8; 64];
        let n = core::cmp::min(bytes.len(), payload.len());
        payload[..n].copy_from_slice(&bytes[..n]);
        Self {
            from,
            to,
            len: n,
            payload,
        }
    }
}

#[derive(Clone, Copy)]
struct IpcSlot {
    used: bool,
    msg: IpcMessage,
}

impl IpcSlot {
    const fn empty() -> Self {
        Self {
            used: false,
            msg: IpcMessage {
                from: IpcEndpoint {
                    id: 0,
                    owner: ProcessHandle { pid: 0 },
                },
                to: IpcEndpoint {
                    id: 0,
                    owner: ProcessHandle { pid: 0 },
                },
                len: 0,
                payload: [0u8; 64],
            },
        }
    }
}

struct IpcState<const N: usize> {
    next_endpoint_id: u32,
    queue: [IpcSlot; N],
}

impl<const N: usize> IpcState<N> {
    const fn new() -> Self {
        Self {
            next_endpoint_id: 1,
            queue: [const { IpcSlot::empty() }; N],
        }
    }
}

pub struct IpcBus<const N: usize> {
    state: Mutex<IpcState<N>>,
}

impl<const N: usize> IpcBus<N> {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(IpcState::new()),
        }
    }

    pub fn register_endpoint(&self, owner: ProcessHandle) -> IpcEndpoint {
        let mut state = self.state.lock();
        let id = state.next_endpoint_id;
        state.next_endpoint_id = state.next_endpoint_id.saturating_add(1);
        IpcEndpoint { id, owner }
    }

    pub fn send(&self, msg: IpcMessage) -> Result<(), IsolationError> {
        let mut state = self.state.lock();
        for slot in &mut state.queue {
            if !slot.used {
                slot.used = true;
                slot.msg = msg;
                return Ok(());
            }
        }
        Err(IsolationError::QueueFull)
    }

    pub fn recv(&self, endpoint: IpcEndpoint) -> Option<IpcMessage> {
        let mut state = self.state.lock();
        for slot in &mut state.queue {
            if slot.used && slot.msg.to.id == endpoint.id {
                let msg = slot.msg;
                *slot = IpcSlot::empty();
                return Some(msg);
            }
        }
        None
    }
}

impl<const N: usize> Default for IpcBus<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockVm {
        installs: Mutex<[CpuContext; 4]>,
        install_count: Mutex<usize>,
        maps: Mutex<usize>,
    }

    impl MockVm {
        const fn new() -> Self {
            Self {
                installs: Mutex::new(
                    [CpuContext {
                        rip: 0,
                        rsp: 0,
                        rflags: 0,
                        cr3: 0,
                    }; 4],
                ),
                install_count: Mutex::new(0),
                maps: Mutex::new(0),
            }
        }
    }

    impl VmContextOps for MockVm {
        fn map_user_region(
            &self,
            _aspace: &AddressSpace,
            _vaddr: u64,
            _paddr: u64,
            _len: u64,
        ) -> Result<(), IsolationError> {
            let mut maps = self.maps.lock();
            *maps += 1;
            Ok(())
        }

        fn install_context(&self, ctx: &CpuContext) -> Result<(), IsolationError> {
            let mut count = self.install_count.lock();
            let mut installs = self.installs.lock();
            if *count < installs.len() {
                installs[*count] = *ctx;
            }
            *count += 1;
            Ok(())
        }
    }

    #[test]
    fn parse_executable_header_accepts_valid_image() {
        let image = [b'S', b'O', b'S', b'X', 1, 0, 0, 0, 8, 0, 0, 0, 16, 0, 0, 0];
        let hdr = parse_executable_header(&image).expect("parse header");
        assert_eq!(hdr.abi.major, 1);
        assert_eq!(hdr.entry_offset, 8);
        assert_eq!(hdr.image_size, 16);
    }

    #[test]
    fn parse_executable_header_rejects_invalid_magic() {
        let image = [0u8; 16];
        assert_eq!(
            parse_executable_header(&image),
            Err(IsolationError::BadExecutable)
        );
    }

    #[test]
    fn spawned_processes_get_isolated_address_spaces() {
        let rt: IsolationRuntime<4, 4> = IsolationRuntime::new();
        let p1 = rt.spawn_process().expect("spawn 1");
        let p2 = rt.spawn_process().expect("spawn 2");

        let a1 = rt.address_space_of(p1).expect("as1");
        let a2 = rt.address_space_of(p2).expect("as2");
        assert_ne!(a1.asid, a2.asid);
        assert_ne!(a1.pml4_phys, a2.pml4_phys);
        assert!(!a1.overlaps(&a2));
    }

    #[test]
    fn process_load_build_map_and_switch_context() {
        let rt: IsolationRuntime<4, 4> = IsolationRuntime::new();
        let p = rt.spawn_process().expect("spawn");
        let image = [b'S', b'O', b'S', b'X', 1, 0, 0, 0, 8, 0, 0, 0, 64, 0, 0, 0];
        assert_eq!(rt.load_executable(p, &image), Ok(()));

        let vm = MockVm::new();
        assert_eq!(rt.map_user_layout(p, &vm), Ok(()));
        let ctx = rt.build_context(p).expect("context");
        assert!(ctx.rip > 0);
        assert!(ctx.rsp > 0);
        assert!(ctx.cr3 > 0);
        assert_eq!(rt.install_context_slot(p, ctx), Ok(()));
        assert_eq!(rt.switch_to(p, &vm), Ok(()));
        assert_eq!(rt.state_of(p), Ok(ProcessState::Running));
    }

    #[test]
    fn switching_without_context_fails() {
        let rt: IsolationRuntime<2, 2> = IsolationRuntime::new();
        let p = rt.spawn_process().expect("spawn");
        let image = [b'S', b'O', b'S', b'X', 1, 0, 0, 0, 4, 0, 0, 0, 16, 0, 0, 0];
        assert_eq!(rt.load_executable(p, &image), Ok(()));
        let vm = MockVm::new();
        assert_eq!(rt.switch_to(p, &vm), Err(IsolationError::BadContext));
    }

    #[test]
    fn process_lifecycle_transitions_work() {
        let rt: IsolationRuntime<2, 2> = IsolationRuntime::new();
        let p = rt.spawn_process().expect("spawn");
        assert_eq!(rt.state_of(p), Ok(ProcessState::Spawned));
        assert_eq!(rt.transition(p, ProcessState::Blocked), Ok(()));
        assert_eq!(rt.transition(p, ProcessState::Exited(7)), Ok(()));
        assert_eq!(rt.state_of(p), Ok(ProcessState::Exited(7)));
        assert_eq!(rt.terminate(p), Ok(()));
        assert_eq!(rt.state_of(p), Err(IsolationError::UnknownProcess));
    }

    #[test]
    fn ipc_bus_routes_message_by_endpoint() {
        let rt: IsolationRuntime<4, 4> = IsolationRuntime::new();
        let p1 = rt.spawn_process().expect("p1");
        let p2 = rt.spawn_process().expect("p2");

        let bus: IpcBus<4> = IpcBus::new();
        let e1 = bus.register_endpoint(p1);
        let e2 = bus.register_endpoint(p2);

        let msg = IpcMessage::from_bytes(e1, e2, b"ping");
        assert_eq!(bus.send(msg), Ok(()));
        let rx = bus.recv(e2).expect("recv");
        assert_eq!(&rx.payload[..rx.len], b"ping");
        assert!(bus.recv(e2).is_none());
    }

    #[test]
    fn ipc_queue_full_returns_error() {
        let rt: IsolationRuntime<2, 2> = IsolationRuntime::new();
        let p1 = rt.spawn_process().expect("p1");
        let p2 = rt.spawn_process().expect("p2");
        let bus: IpcBus<1> = IpcBus::new();
        let e1 = bus.register_endpoint(p1);
        let e2 = bus.register_endpoint(p2);

        assert_eq!(bus.send(IpcMessage::from_bytes(e1, e2, b"one")), Ok(()));
        assert_eq!(
            bus.send(IpcMessage::from_bytes(e1, e2, b"two")),
            Err(IsolationError::QueueFull)
        );
    }
}
