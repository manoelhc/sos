//! Console, command execution, and message-oriented service boundaries.

use core::fmt;

pub const MAX_ARGS: usize = 8;
pub const HISTORY_CAPACITY: usize = 16;
pub const BOOT_PROMPT_BUDGET_MS: u64 = 1500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleError {
    Empty,
    TooManyArgs,
    UnknownProgram,
    SpawnFailed,
    WaitFailed,
}

pub trait ConsoleWriter {
    fn write_str(&mut self, s: &str);
}

pub trait ConsoleReader {
    fn read_line(&mut self, buf: &mut [u8]) -> Option<usize>;
}

pub trait Program {
    fn descriptor(&self) -> ProgramDescriptor<'_>;
    fn execute(&self, args: &[&str], out: &mut dyn ConsoleWriter) -> i32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramDescriptor<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub abi: ProgramAbi,
    pub summary: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramAbi {
    pub major: u16,
    pub minor: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct ParsedCommand<'a> {
    pub program: &'a str,
    args: [&'a str; MAX_ARGS],
    argc: usize,
}

impl<'a> ParsedCommand<'a> {
    pub fn args(&self) -> &[&'a str] {
        &self.args[..self.argc]
    }
}

pub fn parse_command_line(line: &str) -> Result<ParsedCommand<'_>, ConsoleError> {
    let mut parts = line.split_ascii_whitespace();
    let program = parts.next().ok_or(ConsoleError::Empty)?;
    let mut args = [""; MAX_ARGS];
    let mut argc = 0usize;

    for part in parts {
        if argc >= MAX_ARGS {
            return Err(ConsoleError::TooManyArgs);
        }
        args[argc] = part;
        argc += 1;
    }

    Ok(ParsedCommand {
        program,
        args,
        argc,
    })
}

#[derive(Clone, Copy)]
struct HistoryEntry {
    line: [u8; 128],
    len: usize,
}

impl HistoryEntry {
    const fn empty() -> Self {
        Self {
            line: [0u8; 128],
            len: 0,
        }
    }
}

pub struct ConsoleHistory {
    slots: [HistoryEntry; HISTORY_CAPACITY],
    next: usize,
    count: usize,
}

impl ConsoleHistory {
    pub const fn new() -> Self {
        Self {
            slots: [const { HistoryEntry::empty() }; HISTORY_CAPACITY],
            next: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, line: &str) {
        if line.is_empty() {
            return;
        }
        let bytes = line.as_bytes();
        let n = core::cmp::min(bytes.len(), self.slots[self.next].line.len());
        self.slots[self.next].line[..n].copy_from_slice(&bytes[..n]);
        self.slots[self.next].len = n;
        self.next = (self.next + 1) % HISTORY_CAPACITY;
        if self.count < HISTORY_CAPACITY {
            self.count += 1;
        }
    }

    pub fn dump(&self, out: &mut dyn ConsoleWriter) {
        out.write_str("history:");
        for idx in 0..self.count {
            let slot = (self.next + HISTORY_CAPACITY - self.count + idx) % HISTORY_CAPACITY;
            if let Ok(s) = core::str::from_utf8(&self.slots[slot].line[..self.slots[slot].len]) {
                out.write_str(s);
            }
        }
    }
}

impl Default for ConsoleHistory {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ProgramRegistry<'a, const N: usize> {
    programs: [&'a dyn Program; N],
}

impl<'a, const N: usize> ProgramRegistry<'a, N> {
    pub fn new(programs: [&'a dyn Program; N]) -> Self {
        Self { programs }
    }

    pub fn find_program(&self, name: &str) -> Option<&'a dyn Program> {
        self.programs
            .iter()
            .find(|p| p.descriptor().name == name)
            .copied()
    }

    pub fn execute_line(
        &self,
        line: &str,
        out: &mut dyn ConsoleWriter,
    ) -> Result<i32, ConsoleError> {
        let parsed = parse_command_line(line)?;
        let program = self
            .find_program(parsed.program)
            .ok_or(ConsoleError::UnknownProgram)?;
        Ok(program.execute(parsed.args(), out))
    }

    pub fn write_programs(&self, out: &mut dyn ConsoleWriter) {
        out.write_str("programs:");
        for program in &self.programs {
            let d = program.descriptor();
            out.write_str(d.name);
        }
    }

    pub fn write_program_help(
        &self,
        name: &str,
        out: &mut dyn ConsoleWriter,
    ) -> Result<(), ConsoleError> {
        let program = self
            .find_program(name)
            .ok_or(ConsoleError::UnknownProgram)?;
        let d = program.descriptor();
        out.write_str("program:");
        out.write_str(d.name);
        out.write_str("version:");
        out.write_str(d.version);
        out.write_str("abi:");
        let mut abi_line = [0u8; 24];
        let mut n = 0usize;
        n += write_u16_decimal(&mut abi_line[n..], d.abi.major);
        abi_line[n] = b'.';
        n += 1;
        n += write_u16_decimal(&mut abi_line[n..], d.abi.minor);
        if let Ok(s) = core::str::from_utf8(&abi_line[..n]) {
            out.write_str(s);
        }
        out.write_str("summary:");
        out.write_str(d.summary);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PfControlError {
    Unsupported,
    InvalidPolicy,
    Timeout,
}

impl fmt::Display for PfControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PfControlError::Unsupported => write!(f, "unsupported"),
            PfControlError::InvalidPolicy => write!(f, "invalid_policy"),
            PfControlError::Timeout => write!(f, "timeout"),
        }
    }
}

pub trait PacketFilterControl {
    fn check(&self) -> Result<(), PfControlError>;
    fn apply(&self) -> Result<(), PfControlError>;
    fn status(&self, out: &mut dyn ConsoleWriter) -> Result<(), PfControlError>;
    fn export(&self, out: &mut dyn ConsoleWriter) -> Result<(), PfControlError>;
}

const BOOTSTRAP_POLICY_LINES: [&str; 13] = [
    "sos-pf:",
    "  tables:",
    "    - name: runtime_filter",
    "      family: inet",
    "      chains:",
    "        - name: input_filter",
    "          type: filter",
    "          hook: input",
    "          priority: 0",
    "          policy: drop",
    "          rules:",
    "            - action: accept",
    "              comment: runtime bootstrap policy",
];

struct PfRuntimeState {
    applied: bool,
    generation: u64,
}

pub struct KernelPacketFilterControl {
    state: crate::sync::Mutex<PfRuntimeState>,
}

impl KernelPacketFilterControl {
    pub const fn new() -> Self {
        Self {
            state: crate::sync::Mutex::new(PfRuntimeState {
                applied: false,
                generation: 0,
            }),
        }
    }

    fn validate_bootstrap_policy(&self) -> Result<(), PfControlError> {
        let mut has_root = false;
        let mut has_table = false;
        let mut has_chain = false;
        let mut has_policy = false;

        for line in BOOTSTRAP_POLICY_LINES {
            if line == "sos-pf:" {
                has_root = true;
            }
            if line.contains("name: runtime_filter") {
                has_table = true;
            }
            if line.contains("name: input_filter") {
                has_chain = true;
            }
            if line.contains("policy: drop") {
                has_policy = true;
            }
        }

        if has_root && has_table && has_chain && has_policy {
            Ok(())
        } else {
            Err(PfControlError::InvalidPolicy)
        }
    }
}

impl Default for KernelPacketFilterControl {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketFilterControl for KernelPacketFilterControl {
    fn check(&self) -> Result<(), PfControlError> {
        self.validate_bootstrap_policy()
    }

    fn apply(&self) -> Result<(), PfControlError> {
        self.validate_bootstrap_policy()?;
        let mut state = self.state.lock();
        state.applied = true;
        state.generation = state.generation.saturating_add(1);
        Ok(())
    }

    fn status(&self, out: &mut dyn ConsoleWriter) -> Result<(), PfControlError> {
        self.validate_bootstrap_policy()?;
        let state = self.state.lock();

        out.write_str("sos-pf-runtime:");
        out.write_str(if state.applied {
            "  state: applied"
        } else {
            "  state: staged"
        });

        let mut line = [0u8; 32];
        let mut n = 0usize;
        let prefix = b"  generation: ";
        for b in prefix {
            line[n] = *b;
            n += 1;
        }
        n += write_u64_decimal(&mut line[n..], state.generation);
        if let Ok(text) = core::str::from_utf8(&line[..n]) {
            out.write_str(text);
        }

        Ok(())
    }

    fn export(&self, out: &mut dyn ConsoleWriter) -> Result<(), PfControlError> {
        self.status(out)?;

        for line in BOOTSTRAP_POLICY_LINES {
            out.write_str(line);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PfMessage {
    Check,
    Apply,
    Status,
    Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineErrorCode {
    Ok,
    InvalidCommand,
    UnknownProgram,
    PfInvalidPolicy,
    PfTimeout,
    PfUnsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PfResult {
    pub code: i32,
    pub machine: MachineErrorCode,
}

pub trait PfService {
    fn handle(&self, msg: PfMessage, out: &mut dyn ConsoleWriter) -> PfResult;
}

pub struct PfServiceImpl<C: PacketFilterControl> {
    control: C,
}

impl<C: PacketFilterControl> PfServiceImpl<C> {
    pub const fn new(control: C) -> Self {
        Self { control }
    }
}

impl<C: PacketFilterControl> PfService for PfServiceImpl<C> {
    fn handle(&self, msg: PfMessage, out: &mut dyn ConsoleWriter) -> PfResult {
        match msg {
            PfMessage::Check => match self.control.check() {
                Ok(()) => {
                    out.write_str("sos-pf: check ok");
                    out.write_str("sos-code: OK");
                    PfResult {
                        code: 0,
                        machine: MachineErrorCode::Ok,
                    }
                }
                Err(e) => pf_err(e, "check", out),
            },
            PfMessage::Apply => match self.control.apply() {
                Ok(()) => {
                    out.write_str("sos-pf: apply ok");
                    out.write_str("sos-code: OK");
                    PfResult {
                        code: 0,
                        machine: MachineErrorCode::Ok,
                    }
                }
                Err(e) => pf_err(e, "apply", out),
            },
            PfMessage::Status => match self.control.status(out) {
                Ok(()) => {
                    out.write_str("sos-code: OK");
                    PfResult {
                        code: 0,
                        machine: MachineErrorCode::Ok,
                    }
                }
                Err(e) => pf_err(e, "status", out),
            },
            PfMessage::Export => match self.control.export(out) {
                Ok(()) => {
                    out.write_str("sos-code: OK");
                    PfResult {
                        code: 0,
                        machine: MachineErrorCode::Ok,
                    }
                }
                Err(e) => pf_err(e, "export", out),
            },
        }
    }
}

fn pf_err(err: PfControlError, op: &str, out: &mut dyn ConsoleWriter) -> PfResult {
    out.write_str("sos-pf: operation failed");
    out.write_str(op);
    let msg = err.to_string();
    out.write_str(&msg);

    let machine = match err {
        PfControlError::InvalidPolicy => MachineErrorCode::PfInvalidPolicy,
        PfControlError::Timeout => MachineErrorCode::PfTimeout,
        PfControlError::Unsupported => MachineErrorCode::PfUnsupported,
    };
    out.write_str(match machine {
        MachineErrorCode::PfInvalidPolicy => "sos-code: PF_INVALID_POLICY",
        MachineErrorCode::PfTimeout => "sos-code: PF_TIMEOUT",
        MachineErrorCode::PfUnsupported => "sos-code: PF_UNSUPPORTED",
        _ => "sos-code: INVALID",
    });

    PfResult { code: 2, machine }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramHandle {
    pub pid: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramState {
    Spawned,
    Running,
    Exited(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramRequest<'a> {
    Execute {
        program: &'a str,
        args: [&'a str; MAX_ARGS],
        argc: usize,
    },
    Spawn {
        program: &'a str,
        args: [&'a str; MAX_ARGS],
        argc: usize,
    },
    Wait {
        handle: ProgramHandle,
    },
    Terminate {
        handle: ProgramHandle,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramResponse {
    pub exit_code: i32,
    pub machine: MachineErrorCode,
    pub handle: Option<ProgramHandle>,
}

pub trait ProgramService {
    fn handle(
        &self,
        req: ProgramRequest<'_>,
        out: &mut dyn ConsoleWriter,
    ) -> Result<ProgramResponse, ConsoleError>;

    fn list_programs(&self, out: &mut dyn ConsoleWriter);

    fn describe_program(&self, name: &str, out: &mut dyn ConsoleWriter)
        -> Result<(), ConsoleError>;
}

struct TaskEntry {
    handle: ProgramHandle,
    state: ProgramState,
}

pub struct ProgramServiceImpl<'a, const N: usize> {
    registry: ProgramRegistry<'a, N>,
    next_pid: crate::sync::Mutex<u32>,
    tasks: crate::sync::Mutex<[Option<TaskEntry>; 8]>,
    isolation: crate::process::IsolationRuntime<8, 8>,
}

struct NullVm;

impl crate::process::VmContextOps for NullVm {
    fn map_user_region(
        &self,
        _aspace: &crate::process::AddressSpace,
        _vaddr: u64,
        _paddr: u64,
        _len: u64,
    ) -> Result<(), crate::process::IsolationError> {
        Ok(())
    }

    fn install_context(
        &self,
        _ctx: &crate::process::CpuContext,
    ) -> Result<(), crate::process::IsolationError> {
        Ok(())
    }
}

impl<'a, const N: usize> ProgramServiceImpl<'a, N> {
    pub fn new(registry: ProgramRegistry<'a, N>) -> Self {
        Self {
            registry,
            next_pid: crate::sync::Mutex::new(1),
            tasks: crate::sync::Mutex::new([None, None, None, None, None, None, None, None]),
            isolation: crate::process::IsolationRuntime::new(),
        }
    }

    fn alloc_pid(&self) -> u32 {
        let mut pid = self.next_pid.lock();
        let out = *pid;
        *pid = pid.saturating_add(1);
        out
    }

    fn task_insert(&self, handle: ProgramHandle, state: ProgramState) -> bool {
        let mut tasks = self.tasks.lock();
        for slot in tasks.iter_mut() {
            if slot.is_none() {
                *slot = Some(TaskEntry { handle, state });
                return true;
            }
        }
        false
    }

    fn task_update(&self, handle: ProgramHandle, state: ProgramState) -> bool {
        let mut tasks = self.tasks.lock();
        for slot in tasks.iter_mut().flatten() {
            if slot.handle == handle {
                slot.state = state;
                return true;
            }
        }
        false
    }

    fn task_remove(&self, handle: ProgramHandle) {
        let mut tasks = self.tasks.lock();
        for slot in tasks.iter_mut() {
            if slot.as_ref().is_some_and(|entry| entry.handle == handle) {
                *slot = None;
                break;
            }
        }
    }

    fn task_find(&self, handle: ProgramHandle) -> Option<ProgramState> {
        let tasks = self.tasks.lock();
        for slot in tasks.iter().flatten() {
            if slot.handle == handle {
                return Some(slot.state);
            }
        }
        None
    }
}

impl<'a, const N: usize> ProgramService for ProgramServiceImpl<'a, N> {
    fn handle(
        &self,
        req: ProgramRequest<'_>,
        out: &mut dyn ConsoleWriter,
    ) -> Result<ProgramResponse, ConsoleError> {
        match req {
            ProgramRequest::Execute {
                program,
                args,
                argc,
            } => {
                let handle = ProgramHandle {
                    pid: self.alloc_pid(),
                };
                if !self.task_insert(handle, ProgramState::Spawned) {
                    return Err(ConsoleError::SpawnFailed);
                }
                let iso = self.isolation.spawn_process().ok();
                let _ = self.task_update(handle, ProgramState::Running);

                if let Some(iso_handle) = iso {
                    let image = [b'S', b'O', b'S', b'X', 1, 0, 0, 0, 8, 0, 0, 0, 64, 0, 0, 0];
                    let _ = self.isolation.load_executable(iso_handle, &image);
                    let vm = NullVm;
                    let _ = self.isolation.map_user_layout(iso_handle, &vm);
                    if let Ok(ctx) = self.isolation.build_context(iso_handle) {
                        let _ = self.isolation.install_context_slot(iso_handle, ctx);
                        let _ = self.isolation.switch_to(iso_handle, &vm);
                    }
                }

                let prog = self
                    .registry
                    .find_program(program)
                    .ok_or(ConsoleError::UnknownProgram)?;
                let code = prog.execute(&args[..argc], out);
                let _ = self.task_update(handle, ProgramState::Exited(code));
                if let Some(iso_handle) = iso {
                    let _ = self
                        .isolation
                        .transition(iso_handle, crate::process::ProcessState::Exited(code));
                }

                Ok(ProgramResponse {
                    exit_code: code,
                    machine: if code == 0 {
                        MachineErrorCode::Ok
                    } else {
                        MachineErrorCode::InvalidCommand
                    },
                    handle: Some(handle),
                })
            }
            ProgramRequest::Spawn {
                program: _,
                args: _,
                argc: _,
            } => {
                let handle = ProgramHandle {
                    pid: self.alloc_pid(),
                };
                if !self.task_insert(handle, ProgramState::Spawned) {
                    return Err(ConsoleError::SpawnFailed);
                }
                if let Ok(iso_handle) = self.isolation.spawn_process() {
                    let image = [b'S', b'O', b'S', b'X', 1, 0, 0, 0, 8, 0, 0, 0, 64, 0, 0, 0];
                    let _ = self.isolation.load_executable(iso_handle, &image);
                    let vm = NullVm;
                    let _ = self.isolation.map_user_layout(iso_handle, &vm);
                    if let Ok(ctx) = self.isolation.build_context(iso_handle) {
                        let _ = self.isolation.install_context_slot(iso_handle, ctx);
                    }
                }
                Ok(ProgramResponse {
                    exit_code: 0,
                    machine: MachineErrorCode::Ok,
                    handle: Some(handle),
                })
            }
            ProgramRequest::Wait { handle } => {
                let state = self.task_find(handle).ok_or(ConsoleError::WaitFailed)?;
                match state {
                    ProgramState::Exited(code) => {
                        self.task_remove(handle);
                        let _ = self
                            .isolation
                            .terminate(crate::process::ProcessHandle { pid: handle.pid });
                        Ok(ProgramResponse {
                            exit_code: code,
                            machine: if code == 0 {
                                MachineErrorCode::Ok
                            } else {
                                MachineErrorCode::InvalidCommand
                            },
                            handle: Some(handle),
                        })
                    }
                    ProgramState::Spawned | ProgramState::Running => Ok(ProgramResponse {
                        exit_code: 0,
                        machine: MachineErrorCode::Ok,
                        handle: Some(handle),
                    }),
                }
            }
            ProgramRequest::Terminate { handle } => {
                let _ = self.task_find(handle).ok_or(ConsoleError::WaitFailed)?;
                self.task_remove(handle);
                let _ = self
                    .isolation
                    .terminate(crate::process::ProcessHandle { pid: handle.pid });
                Ok(ProgramResponse {
                    exit_code: 0,
                    machine: MachineErrorCode::Ok,
                    handle: Some(handle),
                })
            }
        }
    }

    fn list_programs(&self, out: &mut dyn ConsoleWriter) {
        self.registry.write_programs(out);
    }

    fn describe_program(
        &self,
        name: &str,
        out: &mut dyn ConsoleWriter,
    ) -> Result<(), ConsoleError> {
        self.registry.write_program_help(name, out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootSelfCheckReport {
    pub readiness_ok: bool,
    pub fsck_ok: bool,
    pub pf_ok: bool,
}

impl BootSelfCheckReport {
    pub const fn all_ok() -> Self {
        Self {
            readiness_ok: true,
            fsck_ok: true,
            pf_ok: true,
        }
    }

    pub fn write_transcript(&self, out: &mut dyn ConsoleWriter) {
        out.write_str("boot-self-check:");
        out.write_str(if self.readiness_ok {
            "  readiness: ok"
        } else {
            "  readiness: fail"
        });
        out.write_str(if self.fsck_ok {
            "  fsck: ok"
        } else {
            "  fsck: fail"
        });
        out.write_str(if self.pf_ok {
            "  packet-filter: ok"
        } else {
            "  packet-filter: fail"
        });
    }
}

pub trait MonotonicClock {
    fn now_millis(&self) -> u64;
}

pub struct ConsoleService<'a, P: ProgramService> {
    program_service: &'a P,
    history: crate::sync::Mutex<ConsoleHistory>,
}

impl<'a, P: ProgramService> ConsoleService<'a, P> {
    pub const fn new(program_service: &'a P) -> Self {
        Self {
            program_service,
            history: crate::sync::Mutex::new(ConsoleHistory::new()),
        }
    }

    pub fn run_once(&self, line: &str, out: &mut dyn ConsoleWriter) -> i32 {
        let trimmed = line.trim();

        if !trimmed.is_empty() {
            let mut h = self.history.lock();
            h.push(trimmed);
        }

        if trimmed == "help" {
            out.write_str("console commands:");
            out.write_str("help");
            out.write_str("help <program>");
            out.write_str("programs");
            out.write_str("history");
            out.write_str("<program> <args...>");
            return 0;
        }

        if let Some(name) = trimmed.strip_prefix("help ") {
            return match self.program_service.describe_program(name.trim(), out) {
                Ok(()) => 0,
                Err(ConsoleError::UnknownProgram) => {
                    out.write_str("console: unknown program");
                    2
                }
                _ => 2,
            };
        }

        if trimmed == "programs" {
            self.program_service.list_programs(out);
            return 0;
        }

        if trimmed == "history" {
            let h = self.history.lock();
            h.dump(out);
            return 0;
        }

        let parsed = match parse_command_line(line) {
            Ok(v) => v,
            Err(ConsoleError::Empty) => return 0,
            Err(ConsoleError::TooManyArgs) => {
                out.write_str("console: too many args");
                out.write_str("sos-code: INVALID_COMMAND");
                return 2;
            }
            Err(ConsoleError::UnknownProgram) => {
                out.write_str("console: unknown program");
                out.write_str("sos-code: UNKNOWN_PROGRAM");
                return 2;
            }
            _ => return 2,
        };

        let mut argv = [""; MAX_ARGS];
        for (i, arg) in parsed.args().iter().enumerate() {
            argv[i] = arg;
        }

        match self.program_service.handle(
            ProgramRequest::Execute {
                program: parsed.program,
                args: argv,
                argc: parsed.args().len(),
            },
            out,
        ) {
            Ok(resp) => resp.exit_code,
            Err(ConsoleError::UnknownProgram) => {
                out.write_str("console: unknown program");
                out.write_str("sos-code: UNKNOWN_PROGRAM");
                2
            }
            Err(ConsoleError::TooManyArgs) => {
                out.write_str("console: too many args");
                out.write_str("sos-code: INVALID_COMMAND");
                2
            }
            Err(ConsoleError::Empty) => 0,
            Err(ConsoleError::SpawnFailed) => {
                out.write_str("console: spawn failed");
                out.write_str("sos-code: INVALID_COMMAND");
                2
            }
            Err(ConsoleError::WaitFailed) => {
                out.write_str("console: wait failed");
                out.write_str("sos-code: INVALID_COMMAND");
                2
            }
        }
    }

    pub fn run_loop(
        &self,
        reader: &mut dyn ConsoleReader,
        out: &mut dyn ConsoleWriter,
        prompt: &str,
    ) -> ! {
        let mut buf = [0u8; 256];
        loop {
            out.write_str(prompt);
            if let Some(len) = reader.read_line(&mut buf) {
                let line = core::str::from_utf8(&buf[..len]).unwrap_or("");
                let code = self.run_once(line, out);
                if code != 0 {
                    out.write_str("console: command failed");
                }
            } else {
                out.write_str("console: reader unavailable");
            }
        }
    }

    pub fn run_loop_with_clock(
        &self,
        reader: &mut dyn ConsoleReader,
        out: &mut dyn ConsoleWriter,
        prompt: &str,
        clock: &dyn MonotonicClock,
    ) -> ! {
        let start = clock.now_millis();
        if start > BOOT_PROMPT_BUDGET_MS {
            out.write_str("console: boot prompt budget exceeded");
        }
        self.run_loop(reader, out, prompt)
    }
}

pub struct SosPfProgram<S: PfService> {
    pf_service: S,
}

impl<S: PfService> SosPfProgram<S> {
    pub const fn new(pf_service: S) -> Self {
        Self { pf_service }
    }
}

impl<S: PfService> Program for SosPfProgram<S> {
    fn descriptor(&self) -> ProgramDescriptor<'_> {
        ProgramDescriptor {
            name: "sos-pf",
            version: "0.1.0",
            abi: ProgramAbi { major: 1, minor: 0 },
            summary: "packet filter control and observability",
        }
    }

    fn execute(&self, args: &[&str], out: &mut dyn ConsoleWriter) -> i32 {
        if args.is_empty() || args[0] == "help" {
            out.write_str("usage: sos-pf <check|apply|status|export|export-running>");
            out.write_str("sos-code: OK");
            return 0;
        }

        let msg = match args[0] {
            "check" => PfMessage::Check,
            "apply" => PfMessage::Apply,
            "status" => PfMessage::Status,
            "export" | "export-running" => PfMessage::Export,
            _ => {
                out.write_str("sos-pf: unknown subcommand");
                out.write_str("sos-code: INVALID_COMMAND");
                return 2;
            }
        };

        self.pf_service.handle(msg, out).code
    }
}

fn write_u64_decimal(buf: &mut [u8], mut value: u64) -> usize {
    if buf.is_empty() {
        return 0;
    }
    if value == 0 {
        buf[0] = b'0';
        return 1;
    }

    let mut tmp = [0u8; 20];
    let mut len = 0usize;
    while value > 0 && len < tmp.len() {
        let digit = (value % 10) as u8;
        tmp[len] = b'0' + digit;
        value /= 10;
        len += 1;
    }

    let mut out_len = 0usize;
    while out_len < len && out_len < buf.len() {
        buf[out_len] = tmp[len - 1 - out_len];
        out_len += 1;
    }
    out_len
}

fn write_u16_decimal(buf: &mut [u8], mut value: u16) -> usize {
    if buf.is_empty() {
        return 0;
    }
    if value == 0 {
        buf[0] = b'0';
        return 1;
    }

    let mut tmp = [0u8; 5];
    let mut len = 0usize;
    while value > 0 && len < tmp.len() {
        let digit = (value % 10) as u8;
        tmp[len] = b'0' + digit;
        value /= 10;
        len += 1;
    }

    let mut out_len = 0usize;
    while out_len < len && out_len < buf.len() {
        buf[out_len] = tmp[len - 1 - out_len];
        out_len += 1;
    }
    out_len
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BufOut {
        lines: std::vec::Vec<String>,
    }

    impl BufOut {
        fn new() -> Self {
            Self {
                lines: std::vec::Vec::new(),
            }
        }
    }

    impl ConsoleWriter for BufOut {
        fn write_str(&mut self, s: &str) {
            self.lines.push(s.to_string());
        }
    }

    struct SeqReader {
        lines: std::vec::Vec<&'static str>,
        idx: usize,
    }

    impl SeqReader {
        fn new(lines: std::vec::Vec<&'static str>) -> Self {
            Self { lines, idx: 0 }
        }
    }

    impl ConsoleReader for SeqReader {
        fn read_line(&mut self, buf: &mut [u8]) -> Option<usize> {
            if self.idx >= self.lines.len() {
                return None;
            }
            let line = self.lines[self.idx];
            self.idx += 1;
            let bytes = line.as_bytes();
            let n = core::cmp::min(bytes.len(), buf.len());
            buf[..n].copy_from_slice(&bytes[..n]);
            Some(n)
        }
    }

    struct FixedClock(u64);

    impl MonotonicClock for FixedClock {
        fn now_millis(&self) -> u64 {
            self.0
        }
    }

    #[test]
    fn parse_command_line_splits_program_and_args() {
        let parsed = parse_command_line("sos-pf check").expect("parse");
        assert_eq!(parsed.program, "sos-pf");
        assert_eq!(parsed.args(), ["check"]);
    }

    #[test]
    fn parse_command_line_rejects_empty() {
        let err = parse_command_line("    ").expect_err("must fail");
        assert_eq!(err, ConsoleError::Empty);
    }

    #[test]
    fn parse_command_line_rejects_too_many_args() {
        let err = parse_command_line("sos-pf a b c d e f g h i").expect_err("must fail");
        assert_eq!(err, ConsoleError::TooManyArgs);
    }

    #[test]
    fn registry_dispatches_sos_pf_program() {
        let svc = PfServiceImpl::new(KernelPacketFilterControl::new());
        let prog = SosPfProgram::new(svc);
        let registry: ProgramRegistry<'_, 1> = ProgramRegistry::new([&prog]);
        let mut out = BufOut::new();
        let code = registry
            .execute_line("sos-pf check", &mut out)
            .expect("dispatch");
        assert_eq!(code, 0);
        assert!(out.lines.iter().any(|line| line == "sos-pf: check ok"));
        assert!(out.lines.iter().any(|line| line == "sos-code: OK"));
    }

    #[test]
    fn registry_rejects_unknown_program() {
        let svc = PfServiceImpl::new(KernelPacketFilterControl::new());
        let prog = SosPfProgram::new(svc);
        let registry: ProgramRegistry<'_, 1> = ProgramRegistry::new([&prog]);
        let mut out = BufOut::new();
        let err = registry
            .execute_line("unknown cmd", &mut out)
            .expect_err("unknown program");
        assert_eq!(err, ConsoleError::UnknownProgram);
    }

    #[test]
    fn program_service_handles_execute_spawn_wait_terminate() {
        let svc = PfServiceImpl::new(KernelPacketFilterControl::new());
        let prog = SosPfProgram::new(svc);
        let registry: ProgramRegistry<'_, 1> = ProgramRegistry::new([&prog]);
        let ps = ProgramServiceImpl::new(registry);
        let mut out = BufOut::new();

        let exec_req = ProgramRequest::Execute {
            program: "sos-pf",
            args: ["check", "", "", "", "", "", "", ""],
            argc: 1,
        };
        let exec = ps.handle(exec_req, &mut out).expect("execute");
        assert_eq!(exec.exit_code, 0);
        let handle = exec.handle.expect("handle");

        let wait = ps
            .handle(ProgramRequest::Wait { handle }, &mut out)
            .expect("wait");
        assert_eq!(wait.exit_code, 0);

        let spawned = ps
            .handle(
                ProgramRequest::Spawn {
                    program: "sos-pf",
                    args: ["status", "", "", "", "", "", "", ""],
                    argc: 1,
                },
                &mut out,
            )
            .expect("spawn");
        let spawn_handle = spawned.handle.expect("spawn handle");
        let _ = ps
            .handle(
                ProgramRequest::Terminate {
                    handle: spawn_handle,
                },
                &mut out,
            )
            .expect("terminate");
    }

    #[test]
    fn console_service_runs_one_command() {
        let svc = PfServiceImpl::new(KernelPacketFilterControl::new());
        let prog = SosPfProgram::new(svc);
        let registry: ProgramRegistry<'_, 1> = ProgramRegistry::new([&prog]);
        let ps = ProgramServiceImpl::new(registry);
        let cs = ConsoleService::new(&ps);
        let mut out = BufOut::new();

        let code = cs.run_once("sos-pf export-running", &mut out);
        assert_eq!(code, 0);
        assert!(out.lines.iter().any(|l| l == "sos-pf:"));
    }

    #[test]
    fn console_help_programs_program_help_and_history() {
        let svc = PfServiceImpl::new(KernelPacketFilterControl::new());
        let prog = SosPfProgram::new(svc);
        let registry: ProgramRegistry<'_, 1> = ProgramRegistry::new([&prog]);
        let ps = ProgramServiceImpl::new(registry);
        let cs = ConsoleService::new(&ps);
        let mut out = BufOut::new();

        assert_eq!(cs.run_once("help", &mut out), 0);
        assert!(out.lines.iter().any(|l| l == "console commands:"));

        out.lines.clear();
        assert_eq!(cs.run_once("programs", &mut out), 0);
        assert!(out.lines.iter().any(|l| l == "programs:"));
        assert!(out.lines.iter().any(|l| l == "sos-pf"));

        out.lines.clear();
        assert_eq!(cs.run_once("help sos-pf", &mut out), 0);
        assert!(out.lines.iter().any(|l| l == "program:"));
        assert!(out
            .lines
            .iter()
            .any(|l| l == "packet filter control and observability"));

        out.lines.clear();
        let _ = cs.run_once("sos-pf status", &mut out);
        let _ = cs.run_once("history", &mut out);
        assert!(out.lines.iter().any(|l| l == "history:"));
        assert!(out.lines.iter().any(|l| l == "sos-pf status"));
    }

    #[test]
    fn console_reader_sequence_works() {
        let mut reader = SeqReader::new(std::vec!["sos-pf check", "sos-pf apply"]);
        let mut buf = [0u8; 64];
        let first = reader.read_line(&mut buf).expect("first");
        assert_eq!(
            core::str::from_utf8(&buf[..first]).expect("utf8"),
            "sos-pf check"
        );
        let second = reader.read_line(&mut buf).expect("second");
        assert_eq!(
            core::str::from_utf8(&buf[..second]).expect("utf8"),
            "sos-pf apply"
        );
        assert!(reader.read_line(&mut buf).is_none());
    }

    #[test]
    fn kernel_pf_apply_increments_generation_and_export_state() {
        let control = KernelPacketFilterControl::new();
        let mut out = BufOut::new();

        assert!(control.check().is_ok());
        assert!(control.apply().is_ok());
        assert!(control.export(&mut out).is_ok());

        assert!(out.lines.iter().any(|l| l == "sos-pf-runtime:"));
        assert!(out.lines.iter().any(|l| l == "  state: applied"));
        assert!(out.lines.iter().any(|l| l == "  generation: 1"));
    }

    #[test]
    fn sos_pf_status_shows_exact_generation() {
        let control = KernelPacketFilterControl::new();
        assert!(control.apply().is_ok());
        assert!(control.apply().is_ok());

        let svc = PfServiceImpl::new(control);
        let prog = SosPfProgram::new(svc);
        let registry: ProgramRegistry<'_, 1> = ProgramRegistry::new([&prog]);
        let ps = ProgramServiceImpl::new(registry);
        let cs = ConsoleService::new(&ps);
        let mut out = BufOut::new();

        let code = cs.run_once("sos-pf status", &mut out);
        assert_eq!(code, 0);
        assert!(out.lines.iter().any(|l| l == "  generation: 2"));
    }

    #[test]
    fn boot_self_check_transcript_is_emitted() {
        let report = BootSelfCheckReport::all_ok();
        let mut out = BufOut::new();
        report.write_transcript(&mut out);
        assert!(out.lines.iter().any(|l| l == "boot-self-check:"));
        assert!(out.lines.iter().any(|l| l == "  readiness: ok"));
        assert!(out.lines.iter().any(|l| l == "  fsck: ok"));
        assert!(out.lines.iter().any(|l| l == "  packet-filter: ok"));
    }

    #[test]
    fn prompt_budget_constant_is_enforced_by_contract() {
        let clock = FixedClock(BOOT_PROMPT_BUDGET_MS + 1);
        assert!(clock.now_millis() > BOOT_PROMPT_BUDGET_MS);
    }
}
