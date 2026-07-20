//! Windows compatibility surface for the legacy direct mapped-host fast path.
//!
//! Windows deliberately routes guest filesystem traffic through the AgentOS
//! kernel and the capability-backed `host_dir` mount instead. This module keeps
//! the shared JavaScript RPC implementation type-correct while failing closed
//! if a future change accidentally reaches the Unix-only direct-fd path.

use super::*;
use std::ops::BitOr;

#[derive(Clone, Copy, Debug)]
pub(super) struct OFlag(u32);

impl OFlag {
    pub(super) const O_RDONLY: Self = Self(0);
    pub(super) const O_WRONLY: Self = Self(1);
    pub(super) const O_CREAT: Self = Self(0x40);
    pub(super) const O_TRUNC: Self = Self(0x200);
    pub(super) const O_DIRECTORY: Self = Self(0x1_0000);

    pub(super) const fn empty() -> Self {
        Self(0)
    }

    pub(super) fn from_bits_truncate(bits: i32) -> Self {
        Self(bits as u32)
    }

    pub(super) fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for OFlag {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Mode(u32);

impl Mode {
    pub(super) const fn empty() -> Self {
        Self(0)
    }

    pub(super) fn from_bits_truncate(bits: u32) -> Self {
        Self(bits)
    }
}

#[derive(Debug)]
pub(super) struct AnchoredFd;

impl AnchoredFd {
    pub(super) fn metadata(&self) -> std::io::Result<HostStat> {
        Err(unavailable())
    }

    pub(super) fn read_bytes(&self) -> std::io::Result<Vec<u8>> {
        Err(unavailable())
    }

    pub(super) fn read_to_string(&self) -> std::io::Result<String> {
        Err(unavailable())
    }

    pub(super) fn write_bytes(&self, _data: &[u8]) -> std::io::Result<()> {
        Err(unavailable())
    }

    pub(super) fn set_mode(&self, _mode: u32) -> std::io::Result<()> {
        Err(unavailable())
    }
}

#[derive(Debug)]
pub(super) struct MappedRuntimeOpenedPath {
    pub(super) handle: AnchoredFd,
    pub(super) host_path: PathBuf,
}

#[derive(Debug)]
pub(super) struct MappedRuntimeParentPath {
    pub(super) host_path: PathBuf,
    pub(super) child_name: OsString,
}

fn unavailable() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "direct mapped-host access is disabled on Windows; use a capability-backed host_dir mount",
    )
}

fn unavailable_sidecar(operation: &str, mapped: &MappedRuntimeHostPath) -> SidecarError {
    SidecarError::Io(format!(
        "{operation}: direct mapped-host access is disabled on Windows for {}",
        mapped.guest_path
    ))
}

pub(super) fn open_mapped_runtime_beneath(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
    _flags: OFlag,
    _mode: Mode,
) -> Result<MappedRuntimeOpenedPath, SidecarError> {
    Err(unavailable_sidecar(operation, mapped))
}

pub(super) fn open_mapped_runtime_parent_beneath(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
) -> Result<MappedRuntimeParentPath, SidecarError> {
    Err(unavailable_sidecar(operation, mapped))
}

pub(super) fn mapped_runtime_symlink_metadata(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
) -> Result<HostStat, SidecarError> {
    Err(unavailable_sidecar(operation, mapped))
}

pub(super) fn read_mapped_runtime_link(
    mapped: &MappedRuntimeHostPath,
    _guest_path: &str,
    operation: &str,
) -> Result<PathBuf, SidecarError> {
    Err(unavailable_sidecar(operation, mapped))
}

pub(super) fn create_mapped_runtime_directory(
    _parent: &MappedRuntimeParentPath,
    guest_path: &str,
    _recursive: bool,
) -> Result<(), SidecarError> {
    Err(SidecarError::Io(format!(
        "direct mapped-host mkdir is disabled on Windows for {guest_path}"
    )))
}

pub(super) fn create_mapped_runtime_root_directory(
    mapped: &MappedRuntimeHostPath,
    _recursive: bool,
) -> Result<(), SidecarError> {
    Err(unavailable_sidecar("mkdir", mapped))
}

pub(super) fn ensure_mapped_runtime_parent_dirs(
    mapped: &MappedRuntimeHostPath,
    operation: &str,
) -> Result<(), SidecarError> {
    Err(unavailable_sidecar(operation, mapped))
}

pub(super) fn mapped_runtime_host_path_exists(
    mapped: &MappedRuntimeHostPath,
) -> Result<bool, SidecarError> {
    Err(unavailable_sidecar("exists", mapped))
}

pub(super) fn mapped_child_remove_dir(_parent: &MappedRuntimeParentPath) -> std::io::Result<()> {
    Err(unavailable())
}

pub(super) fn mapped_child_remove_file(_parent: &MappedRuntimeParentPath) -> std::io::Result<()> {
    Err(unavailable())
}

pub(super) fn mapped_child_symlink(
    _parent: &MappedRuntimeParentPath,
    _target: &str,
) -> std::io::Result<()> {
    Err(unavailable())
}

pub(super) fn apply_mapped_child_utimens(
    _parent: &MappedRuntimeParentPath,
    _atime: VirtualUtimeSpec,
    _mtime: VirtualUtimeSpec,
    _context: &str,
) -> Result<(), SidecarError> {
    Err(SidecarError::Io(unavailable().to_string()))
}

pub(super) fn apply_anchored_fd_utimens(
    _handle: &AnchoredFd,
    _atime: VirtualUtimeSpec,
    _mtime: VirtualUtimeSpec,
    _context: &str,
) -> Result<(), SidecarError> {
    Err(SidecarError::Io(unavailable().to_string()))
}

pub(super) fn mapped_child_rename(
    _source: &MappedRuntimeParentPath,
    _destination: &MappedRuntimeParentPath,
) -> std::io::Result<()> {
    Err(unavailable())
}

pub(super) fn mapped_child_rename_at2(
    _source: &MappedRuntimeParentPath,
    _destination: &MappedRuntimeParentPath,
    _flags: u32,
) -> std::io::Result<()> {
    Err(unavailable())
}

pub(super) fn open_mapped_host_fd(
    _kernel: &SidecarKernel,
    _process: &mut ActiveProcess,
    _opened: MappedRuntimeOpenedPath,
    _guest_path: Option<String>,
) -> Result<Value, SidecarError> {
    Err(SidecarError::Io(unavailable().to_string()))
}

pub(super) fn read_mapped_host_fd(
    _mapped: &mut crate::state::ActiveMappedHostFd,
    fd: u32,
    _length: usize,
    _position: Option<u64>,
) -> Result<Value, SidecarError> {
    Err(SidecarError::Io(format!(
        "direct mapped-host fd {fd} is disabled on Windows"
    )))
}

pub(super) fn write_mapped_host_fd(
    _mapped: &mut crate::state::ActiveMappedHostFd,
    fd: u32,
    _contents: &[u8],
    _position: Option<u64>,
) -> Result<Value, SidecarError> {
    Err(SidecarError::Io(format!(
        "direct mapped-host fd {fd} is disabled on Windows"
    )))
}

pub(super) fn write_all_mapped_host_fd(
    _mapped: &mut crate::state::ActiveMappedHostFd,
    fd: u32,
    _contents: &[u8],
    _position: Option<u64>,
) -> Result<usize, SidecarError> {
    Err(SidecarError::Io(format!(
        "direct mapped-host fd {fd} is disabled on Windows"
    )))
}
