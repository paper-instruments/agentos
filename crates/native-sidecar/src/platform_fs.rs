//! Small host-filesystem compatibility helpers.
//!
//! Guest POSIX metadata remains virtualized by the AgentOS kernel. These
//! helpers only maintain the private runtime shadow tree used by the embedded
//! JavaScript and Python engines.

use std::fs;
use std::io;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub(crate) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(path)?.permissions();
        // Windows has no POSIX mode bits. Preserve the security-relevant write
        // distinction for the private shadow while the guest kernel retains
        // the complete virtual mode.
        permissions.set_readonly(mode & 0o222 == 0);
        fs::set_permissions(path, permissions)
    }
}

pub(crate) fn metadata_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        metadata.permissions().mode()
    }
    #[cfg(windows)]
    {
        if metadata.is_dir() {
            if metadata.permissions().readonly() {
                0o555
            } else {
                0o777
            }
        } else if metadata.permissions().readonly() {
            0o444
        } else {
            0o666
        }
    }
}

pub(crate) fn create_symlink(target: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        let target = target.as_ref();
        let link = link.as_ref();
        let resolved_target = if target.is_absolute() {
            target.to_path_buf()
        } else {
            link.parent().unwrap_or_else(|| Path::new(".")).join(target)
        };
        if resolved_target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        }
    }
}

pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700).create(path)?;
        set_mode(path, 0o700)
    }
    #[cfg(windows)]
    {
        // The directory lives beneath the per-user temp root. Host-directory
        // mounts themselves are independently confined by directory handles.
        fs::create_dir(path)
    }
}
