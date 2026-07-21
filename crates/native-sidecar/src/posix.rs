//! POSIX ABI constants used by the virtual guest on Windows.
//!
//! These are guest-visible Linux/POSIX numbers, not Win32 process-control
//! values. AgentOS handles them inside its virtual kernel and never forwards
//! them to an arbitrary Windows PID.

pub(crate) const EPERM: i32 = 1;
pub(crate) const ENOENT: i32 = 2;
pub(crate) const EACCES: i32 = 13;
pub(crate) const EEXIST: i32 = 17;
pub(crate) const EXDEV: i32 = 18;
pub(crate) const ENOTDIR: i32 = 20;
pub(crate) const EISDIR: i32 = 21;
pub(crate) const EINVAL: i32 = 22;
pub(crate) const ENAMETOOLONG: i32 = 36;
pub(crate) const ENOSYS: i32 = 38;
pub(crate) const ENOTEMPTY: i32 = 39;
pub(crate) const ELOOP: i32 = 40;
pub(crate) const EROFS: i32 = 30;
pub(crate) const ENOTSUP: i32 = 95;
pub(crate) const EOPNOTSUPP: i32 = 95;
pub(crate) const ENOPROTOOPT: i32 = 92;
pub(crate) const EADDRINUSE: i32 = 98;
pub(crate) const EADDRNOTAVAIL: i32 = 99;
pub(crate) const ECONNRESET: i32 = 104;
pub(crate) const ENOTCONN: i32 = 107;
pub(crate) const ETIMEDOUT: i32 = 110;
pub(crate) const ECONNREFUSED: i32 = 111;
pub(crate) const EHOSTUNREACH: i32 = 113;
pub(crate) const ENETUNREACH: i32 = 101;
pub(crate) const EDESTADDRREQ: i32 = 89;
pub(crate) const EPIPE: i32 = 32;
pub(crate) const EBADF: i32 = 9;
pub(crate) const O_RDONLY: i32 = 0;
pub(crate) const O_ACCMODE: i32 = 3;
pub(crate) const O_WRONLY: i32 = 1;
pub(crate) const O_RDWR: i32 = 2;
pub(crate) const O_CREAT: i32 = 0x40;
pub(crate) const O_TRUNC: i32 = 0x200;
pub(crate) const O_APPEND: i32 = 0x400;
pub(crate) const X_OK: i32 = 1;
pub(crate) const W_OK: i32 = 2;
pub(crate) const R_OK: i32 = 4;

pub(crate) const SIGHUP: i32 = 1;
pub(crate) const SIGINT: i32 = 2;
pub(crate) const SIGKILL: i32 = 9;
pub(crate) const SIGUSR1: i32 = 10;
pub(crate) const SIGALRM: i32 = 14;
pub(crate) const SIGTERM: i32 = 15;
pub(crate) const SIGCHLD: i32 = 17;
pub(crate) const SIGCONT: i32 = 18;
pub(crate) const SIGSTOP: i32 = 19;
pub(crate) const SIGTSTP: i32 = 20;
pub(crate) const SIGTTIN: i32 = 21;
pub(crate) const SIGTTOU: i32 = 22;
pub(crate) const SIGURG: i32 = 23;
pub(crate) const SIGWINCH: i32 = 28;

pub(crate) const UTIME_NOW: i64 = 1_073_741_823;
pub(crate) const UTIME_OMIT: i64 = 1_073_741_822;
