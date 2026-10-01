use libkernel::{
    error::{KernelError, Result}, 
    memory::address::TUA, 
    sync::once_lock::OnceLock,
};


use alloc::{
    string::String,
    boxed::Box,
};
use core::fmt::Write;
use core::time::Duration;

use crate::drivers::timer::now;
use crate::arch::ArchImpl;
use crate::sync::SpinLock;
use crate::memory::uaccess::copy_to_user_slice;

const LOG_CAPACITY: usize = 16;

#[derive(Clone, Copy)]
struct LogRecord {
    timestamp: u64,
    number: u32,
}

const EMPTY_RECORD: LogRecord = LogRecord { timestamp: 0, number: 0 };

struct RingBuffer {
    records: Box<[LogRecord; LOG_CAPACITY]>,
    next: usize,
    len: usize,
    dropped: usize,
}

impl RingBuffer {
    pub fn new() -> Self {
            Self {
                records: Box::new([EMPTY_RECORD; LOG_CAPACITY]),
                next: 0,
                len: 0,
                dropped: 0,
            }
        }

    pub fn push(&mut self, record: LogRecord) {
        if self.len == LOG_CAPACITY {
            self.dropped += 1;
        } else {
            self.len += 1;
        }

        self.records[self.next] = record;
        self.next = (self.next + 1) % LOG_CAPACITY;
    }

    pub fn get(&self, index: usize) -> Option<&LogRecord> {
        if index >= self.len {
            return None;
        }

        // If not full, oldest starts at 0.
        // If full, `next` points to the oldest record.
        let oldest = if self.len == LOG_CAPACITY {
            self.next
        } 
        else {
            0
        };

        let real_index = (oldest + index) % LOG_CAPACITY;

        Some(&self.records[real_index])
    }

    pub fn render(&self) -> String {
        let mut output = String::new();

        writeln!(
            &mut output,
            "{} earlier records dropped",
            self.dropped
        )
        .unwrap();

        for i in 0..self.len {
            if let Some(record) = self.get(i) {
                let seconds = record.timestamp / 1_000_000;
                let micros = record.timestamp % 1_000_000;

                writeln!(
                    &mut output,
                    "[ {}.{:06}] syscall {}",
                    seconds,
                    micros,
                    record.number,
                )
                .unwrap();
            }
        }

        output
    }

    pub fn clear(&mut self) {
        self.next = 0;
        self.len = 0;
        self.dropped = 0;
    }

}

static LOG_BUFFER: OnceLock<SpinLock<RingBuffer>, ArchImpl> = OnceLock::new();

pub fn record_syscall(nr: u32) {
    let current: Duration = now().expect("system timer not running").into();
    let current = current.as_micros() as u64;
    let log_buffer = LOG_BUFFER.get_or_init(|| SpinLock::new(RingBuffer::new()));
    log_buffer.lock_save_irq().push(LogRecord { timestamp: current, number: nr });
}


pub async fn sys_syslog(type_: i32, buf: TUA<u8>, len: usize) -> Result<usize> {
    let log_buffer = LOG_BUFFER.get_or_init(|| SpinLock::new(RingBuffer::new()));

    match type_ {
        3 => {
            let output = {
                let guard = log_buffer.lock_save_irq();
                guard.render()
            };

            let bytes = output.as_bytes();
            let written = core::cmp::min(len, bytes.len());

            copy_to_user_slice(&bytes[..written],buf.to_untyped()).await?;
            Ok(written)
        }
        5 => {
            log_buffer.lock_save_irq().clear();
            Ok(0)
        }
        10 => {
            let size = {
                let guard = log_buffer.lock_save_irq();
                guard.render().len()
            };
            Ok(size)
        }
        _ => Err(KernelError::InvalidValue), // becomes -EINVAL
    }
}

#[allow(dead_code)]
fn syscall_name(nr: u32) -> &'static str {
    match nr {
        0x05 => "setxattr",
        0x06 => "lsetxattr",
        0x07 => "fsetxattr",
        0x08 => "getxattr",
        0x09 => "lgetxattr",
        0x0a => "fgetxattr",
        0x0b => "listxattr",
        0x0c => "llistxattr",
        0x0d => "flistxattr",
        0x0e => "removexattr",
        0x0f => "lremovexattr",
        0x10 => "fremovexattr",
        0x11 => "getcwd",
        0x17 => "dup",
        0x18 => "dup3",
        0x19 => "fcntl",
        0x1d => "ioctl",
        0x20 => "flock",
        0x22 => "mkdirat",
        0x23 => "unlinkat",
        0x24 => "symlinkat",
        0x25 => "linkat",
        0x26 => "renameat",
        0x2b => "statfs",
        0x2c => "fstatfs",
        0x2d => "truncate",
        0x2e => "ftruncate",
        0x30 => "faccessat",
        0x31 => "chdir",
        0x32 => "fchdir",
        0x33 => "chroot",
        0x34 => "fchmod",
        0x35 => "fchmodat",
        0x36 => "fchownat",
        0x37 => "fchown",
        0x38 => "openat",
        0x39 => "close",
        0x3b => "pipe2",
        0x3d => "getdents64",
        0x3e => "lseek",
        0x3f => "read",
        0x40 => "write",
        0x41 => "readv",
        0x42 => "writev",
        0x43 => "pread64",
        0x44 => "pwrite64",
        0x45 => "preadv",
        0x46 => "pwritev",
        0x47 => "sendfile",
        0x48 => "pselect6",
        0x49 => "ppoll",
        0x4e => "readlinkat",
        0x4f => "newfstatat",
        0x50 => "fstat",
        0x51 => "sync",
        0x52 => "fsync",
        0x53 => "fdatasync",
        0x58 => "utimensat",
        0x5a => "capget",
        0x5b => "capset",
        0x5d => "exit",
        0x5e => "exit_group",
        0x5f => "waitid",
        0x60 => "set_tid_address",
        0x62 => "futex",
        0x63 => "set_robust_list",
        0x65 => "nanosleep",
        0x70 => "clock_settime",
        0x71 => "clock_gettime",
        0x73 => "clock_nanosleep",
        0x74 => "syslog",
        0x75 => "ptrace",
        0x7b => "sched_setparam",
        0x7c => "sched_yield",
        0x81 => "kill",
        0x82 => "tkill",
        0x84 => "sigaltstack",
        0x86 => "rt_sigaction",
        0x87 => "rt_sigprocmask",
        0x8b => "rt_sigreturn",
        0x8e => "reboot",
        0x94 => "getresuid",
        0x96 => "getresgid",
        0x97 => "setfsuid",
        0x98 => "setfsgid",
        0x9a => "setpgid",
        0x9b => "getpgid",
        0x9c => "getsid",
        0x9d => "setsid",
        0xa0 => "uname",
        0xa1 => "sethostname",
        0xa3 => "setrlimit",
        0xa6 => "umask",
        0xa7 => "prctl",
        0xa9 => "gettimeofday",
        0xaa => "settimeofday",
        0xac => "getpid",
        0xad => "getppid",
        0xae => "getuid",
        0xaf => "geteuid",
        0xb0 => "getgid",
        0xb1 => "getegid",
        0xb2 => "gettid",
        0xb3 => "sysinfo",
        0xc6 => "socket",
        0xd6 => "brk",
        0xd7 => "munmap",
        0xdc => "clone",
        0xdd => "execve",
        0xde => "mmap",
        0xdf => "fadvise64",
        0xe2 => "mprotect",
        0xe8 => "mincore",
        0xe9 => "madvise",
        0x104 => "wait4",
        0x105 => "prlimit64",
        0x108 => "name_to_handle_at",
        0x109 => "open_by_handle_at",
        0x10b => "syncfs",
        0x10e => "process_vm_readv",
        0x114 => "renameat2",
        0x116 => "getrandom",
        0x11d => "copy_file_range",
        0x11e => "preadv2",
        0x11f => "pwritev2",
        0x123 => "statx",
        0x125 => "membarrier",
        0x1b4 => "close_range",
        0x1b7 => "faccessat2",
        0x1b8 => "process_madvise",
        _ => "unknown",
    }
}