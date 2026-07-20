//! Windows host-directory mount backed by `cap-std` directory capabilities.
//!
//! A guest path is never joined to an ambient host path. The mount root is
//! opened once as a capability and every operation stays relative to that
//! handle. `cap-std` rejects absolute paths, `..` escapes, reserved Windows
//! device names, and reparse-point/symlink traversal outside the capability.

use agentos_execution::{
    GuestModuleReader, LocalModuleResolutionCache, ModuleFsReader, ModuleResolveMode,
    ModuleResolver,
};
use agentos_kernel::mount_plugin::{
    FileSystemPluginFactory, OpenFileSystemPluginRequest, PluginError,
};
use agentos_kernel::mount_table::{
    MountedFileSystem, MountedVirtualFileSystem, ReadOnlyFileSystem,
};
use agentos_kernel::resource_accounting::DEFAULT_MAX_PREAD_BYTES;
use agentos_kernel::vfs::{
    normalize_path, VfsError, VfsResult, VirtualDirEntry, VirtualFileSystem, VirtualStat,
    VirtualTimeSpec, VirtualUtimeSpec,
};
use cap_std::ambient_authority;
use cap_std::fs::{Dir, File, FileExt, Metadata, OpenOptions};
use serde::Deserialize;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use vfs::posix::TarFileSystem;

const MAX_HOST_DIR_READ_BYTES: usize = DEFAULT_MAX_PREAD_BYTES;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostDirMountConfig {
    host_path: String,
    read_only: Option<bool>,
}

#[derive(Debug)]
pub(crate) struct HostDirMountPlugin;

pub(crate) trait HostDirReadLimitContext {
    fn host_dir_max_read_bytes(&self) -> Option<usize>;
}

impl HostDirReadLimitContext for () {
    fn host_dir_max_read_bytes(&self) -> Option<usize> {
        Some(MAX_HOST_DIR_READ_BYTES)
    }
}

impl<Context> FileSystemPluginFactory<Context> for HostDirMountPlugin
where
    Context: HostDirReadLimitContext,
{
    fn plugin_id(&self) -> &'static str {
        "host_dir"
    }

    fn open(
        &self,
        request: OpenFileSystemPluginRequest<'_, Context>,
    ) -> Result<Box<dyn MountedFileSystem>, PluginError> {
        let config: HostDirMountConfig = serde_json::from_value(request.config.clone())
            .map_err(|error| PluginError::invalid_input(error.to_string()))?;
        let filesystem = HostDirFilesystem::new_with_read_limit(
            &config.host_path,
            request.context.host_dir_max_read_bytes(),
        )?;
        let mounted = MountedVirtualFileSystem::new(filesystem);
        if config.read_only.unwrap_or(false) {
            Ok(Box::new(ReadOnlyFileSystem::new(mounted)))
        } else {
            Ok(Box::new(mounted))
        }
    }
}

#[derive(Clone)]
pub(crate) struct HostDirFilesystem {
    root: Arc<Dir>,
    max_read_bytes: Option<usize>,
}

impl std::fmt::Debug for HostDirFilesystem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HostDirFilesystem")
            .field("root", &"<directory capability>")
            .field("max_read_bytes", &self.max_read_bytes)
            .finish()
    }
}

impl HostDirFilesystem {
    #[allow(dead_code)]
    pub(crate) fn new(host_path: impl AsRef<Path>) -> VfsResult<Self> {
        Self::new_with_read_limit(host_path, Some(MAX_HOST_DIR_READ_BYTES))
    }

    pub(crate) fn new_with_read_limit(
        host_path: impl AsRef<Path>,
        max_read_bytes: Option<usize>,
    ) -> VfsResult<Self> {
        let display = host_path.as_ref().to_string_lossy();
        let root = Dir::open_ambient_dir(host_path.as_ref(), ambient_authority())
            .map_err(|error| io_error_to_vfs("open", &display, error))?;
        let metadata = root
            .dir_metadata()
            .map_err(|error| io_error_to_vfs("stat", &display, error))?;
        if !metadata.is_dir() {
            return Err(VfsError::new(
                "ENOTDIR",
                format!("host_dir root is not a directory: {display}"),
            ));
        }
        Ok(Self {
            root: Arc::new(root),
            max_read_bytes,
        })
    }

    fn relative_path(&self, path: &str) -> VfsResult<(String, PathBuf)> {
        let normalized = normalize_path(path);
        let mut relative = PathBuf::new();
        for segment in normalized.split('/').filter(|segment| !segment.is_empty()) {
            // Backslashes and colons acquire path-separator/drive semantics on
            // Windows. They are not valid separators in the guest POSIX path.
            if segment == "." || segment == ".." || segment.contains(['\\', ':', '\0']) {
                return Err(VfsError::access_denied(
                    "open",
                    path,
                    Some("invalid Windows host-mount path component"),
                ));
            }
            relative.push(segment);
        }
        if relative.as_os_str().is_empty() {
            relative.push(".");
        }
        Ok((normalized, relative))
    }

    fn open_read(&self, path: &str) -> VfsResult<File> {
        let (_, relative) = self.relative_path(path)?;
        self.root
            .open(&relative)
            .map_err(|error| io_error_to_vfs("open", path, error))
    }

    fn open_write(&self, path: &str, create: bool, truncate: bool) -> VfsResult<File> {
        let (_, relative) = self.relative_path(path)?;
        let mut options = OpenOptions::new();
        options.write(true).create(create).truncate(truncate);
        self.root
            .open_with(&relative, &options)
            .map_err(|error| io_error_to_vfs("open", path, error))
    }

    fn check_read_length(&self, path: &str, length: usize) -> VfsResult<()> {
        if self.max_read_bytes.is_some_and(|limit| length > limit) {
            return Err(VfsError::new(
                "EINVAL",
                format!("read length {length} exceeds host_dir limit: {path}"),
            ));
        }
        Ok(())
    }

    fn stat_from_metadata(metadata: &Metadata) -> VirtualStat {
        let is_symlink = metadata.is_symlink();
        let is_directory = metadata.is_dir();
        let mode_type = if is_symlink {
            0o120000
        } else if is_directory {
            0o040000
        } else {
            0o100000
        };
        let permissions = if is_symlink {
            0o777
        } else if is_directory {
            // The Windows read-only attribute on a directory does not model
            // POSIX search/write bits. Mount policy and the host ACL remain
            // authoritative, while guest directories must stay traversable.
            0o777
        } else if metadata.permissions().readonly() {
            0o444
        } else {
            0o666
        };
        let (atime_ms, atime_nsec) = cap_time_parts(metadata.accessed().ok());
        let (mtime_ms, mtime_nsec) = cap_time_parts(metadata.modified().ok());
        let (birthtime_ms, _) = cap_time_parts(metadata.created().ok());
        VirtualStat {
            mode: mode_type | permissions,
            size: metadata.len(),
            blocks: metadata.len().div_ceil(512),
            dev: 0,
            rdev: 0,
            is_directory,
            is_symbolic_link: is_symlink,
            atime_ms,
            atime_nsec,
            mtime_ms,
            mtime_nsec,
            ctime_ms: mtime_ms,
            ctime_nsec: mtime_nsec,
            birthtime_ms,
            ino: 0,
            nlink: 1,
            uid: 1000,
            gid: 1000,
        }
    }

    fn virtual_path(path: &Path) -> VfsResult<String> {
        let mut segments = Vec::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(segment) => segments.push(segment.to_string_lossy().into_owned()),
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(VfsError::access_denied(
                        "realpath",
                        &path.to_string_lossy(),
                        Some("path escapes host directory"),
                    ));
                }
            }
        }
        Ok(if segments.is_empty() {
            String::from("/")
        } else {
            format!("/{}", segments.join("/"))
        })
    }

    fn link_target(&self, target: &str, link_path: &str) -> VfsResult<(PathBuf, bool)> {
        let link_virtual = normalize_path(link_path);
        let target_virtual = if target.starts_with('/') {
            normalize_path(target)
        } else {
            let parent = link_virtual
                .rsplit_once('/')
                .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
                .unwrap_or("/");
            normalize_path(&format!("{parent}/{target}"))
        };
        let (_, target_relative) = self.relative_path(&target_virtual)?;
        let (_, link_relative) = self.relative_path(&link_virtual)?;
        let link_parent = link_relative.parent().unwrap_or_else(|| Path::new("."));
        let relative_target = relative_path(link_parent, &target_relative);
        let is_dir = self
            .root
            .metadata(&target_relative)
            .is_ok_and(|metadata| metadata.is_dir());
        Ok((relative_target, is_dir))
    }

    fn set_file_times(&self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()> {
        let (_, relative) = self.relative_path(path)?;
        let atime = filetime::FileTime::from_unix_time(
            i64::try_from(atime_ms / 1_000).unwrap_or(i64::MAX),
            ((atime_ms % 1_000) * 1_000_000) as u32,
        );
        let mtime = filetime::FileTime::from_unix_time(
            i64::try_from(mtime_ms / 1_000).unwrap_or(i64::MAX),
            ((mtime_ms % 1_000) * 1_000_000) as u32,
        );
        let metadata = self
            .root
            .metadata(&relative)
            .map_err(|error| io_error_to_vfs("utimes", path, error))?;
        let file = if metadata.is_dir() {
            self.root
                .open_dir(&relative)
                .map(Dir::into_std_file)
                .map_err(|error| io_error_to_vfs("utimes", path, error))?
        } else {
            // `SetFileTime` needs a handle with write-attributes access on
            // Windows. `Dir::open` returns a read handle, which makes utimes
            // fail with ERROR_ACCESS_DENIED even for a writable file.
            self.open_write(path, false, false)?.into_std()
        };
        filetime::set_file_handle_times(&file, Some(atime), Some(mtime))
            .map_err(|error| io_error_to_vfs("utimes", path, error))
    }
}

impl VirtualFileSystem for HostDirFilesystem {
    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        let mut file = self.open_read(path)?;
        let size = file
            .metadata()
            .map_err(|error| io_error_to_vfs("stat", path, error))?
            .len();
        if self.max_read_bytes.is_some_and(|limit| size > limit as u64) {
            return Err(VfsError::new(
                "EINVAL",
                format!("file size {size} exceeds host_dir read limit: {path}"),
            ));
        }
        let mut bytes = Vec::new();
        match self.max_read_bytes {
            Some(limit) => Read::by_ref(&mut file)
                .take((limit as u64).saturating_add(1))
                .read_to_end(&mut bytes),
            None => file.read_to_end(&mut bytes),
        }
        .map_err(|error| io_error_to_vfs("read", path, error))?;
        self.check_read_length(path, bytes.len())?;
        Ok(bytes)
    }

    fn read_dir(&mut self, path: &str) -> VfsResult<Vec<String>> {
        Ok(self
            .read_dir_with_types(path)?
            .into_iter()
            .map(|entry| entry.name)
            .collect())
    }

    fn read_dir_with_types(&mut self, path: &str) -> VfsResult<Vec<VirtualDirEntry>> {
        let (_, relative) = self.relative_path(path)?;
        let mut entries = self
            .root
            .read_dir(&relative)
            .map_err(|error| io_error_to_vfs("readdir", path, error))?
            .map(|entry| {
                let entry = entry.map_err(|error| io_error_to_vfs("readdir", path, error))?;
                let kind = entry
                    .file_type()
                    .map_err(|error| io_error_to_vfs("readdir", path, error))?;
                Ok(VirtualDirEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    is_directory: kind.is_dir(),
                    is_symbolic_link: kind.is_symlink(),
                })
            })
            .collect::<VfsResult<Vec<_>>>()?;
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    fn write_file(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<()> {
        self.write_file_with_mode(path, content, None)
    }

    fn write_file_with_mode(
        &mut self,
        path: &str,
        content: impl Into<Vec<u8>>,
        _mode: Option<u32>,
    ) -> VfsResult<()> {
        let (_, relative) = self.relative_path(path)?;
        if let Some(parent) = relative.parent() {
            self.root
                .create_dir_all(parent)
                .map_err(|error| io_error_to_vfs("mkdir", path, error))?;
        }
        let mut file = self.open_write(path, true, true)?;
        file.write_all(&content.into())
            .map_err(|error| io_error_to_vfs("write", path, error))
    }

    fn create_file_exclusive(&mut self, path: &str, content: impl Into<Vec<u8>>) -> VfsResult<()> {
        let (_, relative) = self.relative_path(path)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut file = self
            .root
            .open_with(relative, &options)
            .map_err(|error| io_error_to_vfs("open", path, error))?;
        file.write_all(&content.into())
            .map_err(|error| io_error_to_vfs("write", path, error))
    }

    fn create_dir(&mut self, path: &str) -> VfsResult<()> {
        let (_, relative) = self.relative_path(path)?;
        self.root
            .create_dir(relative)
            .map_err(|error| io_error_to_vfs("mkdir", path, error))
    }

    fn mkdir(&mut self, path: &str, recursive: bool) -> VfsResult<()> {
        let (_, relative) = self.relative_path(path)?;
        let result = if recursive {
            self.root.create_dir_all(relative)
        } else {
            self.root.create_dir(relative)
        };
        result.map_err(|error| io_error_to_vfs("mkdir", path, error))
    }

    fn exists(&self, path: &str) -> bool {
        self.relative_path(path)
            .ok()
            .is_some_and(|(_, relative)| self.root.metadata(relative).is_ok())
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        let (_, relative) = self.relative_path(path)?;
        self.root
            .metadata(relative)
            .map(|metadata| Self::stat_from_metadata(&metadata))
            .map_err(|error| io_error_to_vfs("stat", path, error))
    }

    fn remove_file(&mut self, path: &str) -> VfsResult<()> {
        let (_, relative) = self.relative_path(path)?;
        self.root
            .remove_file(relative)
            .map_err(|error| io_error_to_vfs("unlink", path, error))
    }

    fn remove_dir(&mut self, path: &str) -> VfsResult<()> {
        let (_, relative) = self.relative_path(path)?;
        self.root
            .remove_dir(relative)
            .map_err(|error| io_error_to_vfs("rmdir", path, error))
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let (_, old_relative) = self.relative_path(old_path)?;
        let (_, new_relative) = self.relative_path(new_path)?;
        self.root
            .rename(old_relative, &self.root, new_relative)
            .map_err(|error| io_error_to_vfs("rename", old_path, error))
    }

    fn realpath(&self, path: &str) -> VfsResult<String> {
        let (_, relative) = self.relative_path(path)?;
        let canonical = self
            .root
            .canonicalize(relative)
            .map_err(|error| io_error_to_vfs("realpath", path, error))?;
        Self::virtual_path(&canonical)
    }

    fn symlink(&mut self, target: &str, link_path: &str) -> VfsResult<()> {
        let (_, link_relative) = self.relative_path(link_path)?;
        let (relative_target, target_is_dir) = self.link_target(target, link_path)?;
        let result = if target_is_dir {
            self.root.symlink_dir(relative_target, link_relative)
        } else {
            self.root.symlink_file(relative_target, link_relative)
        };
        result.map_err(|error| io_error_to_vfs("symlink", link_path, error))
    }

    fn read_link(&self, path: &str) -> VfsResult<String> {
        let (_, relative) = self.relative_path(path)?;
        let target = self
            .root
            .read_link(&relative)
            .map_err(|error| io_error_to_vfs("readlink", path, error))?;
        let resolved = lexical_join(relative.parent().unwrap_or_else(|| Path::new(".")), &target)?;
        Self::virtual_path(&resolved)
    }

    fn lstat(&self, path: &str) -> VfsResult<VirtualStat> {
        let (_, relative) = self.relative_path(path)?;
        self.root
            .symlink_metadata(relative)
            .map(|metadata| Self::stat_from_metadata(&metadata))
            .map_err(|error| io_error_to_vfs("lstat", path, error))
    }

    fn link(&mut self, old_path: &str, new_path: &str) -> VfsResult<()> {
        let (_, old_relative) = self.relative_path(old_path)?;
        let (_, new_relative) = self.relative_path(new_path)?;
        self.root
            .hard_link(old_relative, &self.root, new_relative)
            .map_err(|error| io_error_to_vfs("link", new_path, error))
    }

    fn chmod(&mut self, path: &str, mode: u32) -> VfsResult<()> {
        let (_, relative) = self.relative_path(path)?;
        let mut permissions = self
            .root
            .metadata(&relative)
            .map_err(|error| io_error_to_vfs("chmod", path, error))?
            .permissions();
        permissions.set_readonly(mode & 0o222 == 0);
        self.root
            .set_permissions(relative, permissions)
            .map_err(|error| io_error_to_vfs("chmod", path, error))
    }

    fn chown(&mut self, path: &str, _uid: u32, _gid: u32) -> VfsResult<()> {
        // NTFS ownership is not the guest's virtual POSIX ownership. Keep the
        // host ACL/owner unchanged while reporting the mounted entry as owned
        // by the virtual agent user.
        self.lstat(path).map(|_| ())
    }

    fn utimes(&mut self, path: &str, atime_ms: u64, mtime_ms: u64) -> VfsResult<()> {
        self.set_file_times(path, atime_ms, mtime_ms)
    }

    fn utimes_spec(
        &mut self,
        path: &str,
        atime: VirtualUtimeSpec,
        mtime: VirtualUtimeSpec,
        follow_symlinks: bool,
    ) -> VfsResult<()> {
        if !follow_symlinks {
            return Err(VfsError::new(
                "EOPNOTSUPP",
                format!("symlink timestamps are not supported on Windows: {path}"),
            ));
        }
        let existing = self.stat(path)?;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
            .unwrap_or_default();
        let resolve = |spec: VirtualUtimeSpec, current_ms: u64| match spec {
            VirtualUtimeSpec::Now => now_ms,
            VirtualUtimeSpec::Omit => current_ms,
            VirtualUtimeSpec::Set(VirtualTimeSpec { sec, nsec }) => (sec.max(0) as u64)
                .saturating_mul(1_000)
                .saturating_add(u64::from(nsec) / 1_000_000),
        };
        self.set_file_times(
            path,
            resolve(atime, existing.atime_ms),
            resolve(mtime, existing.mtime_ms),
        )
    }

    fn truncate(&mut self, path: &str, length: u64) -> VfsResult<()> {
        self.open_write(path, false, false)?
            .set_len(length)
            .map_err(|error| io_error_to_vfs("truncate", path, error))
    }

    fn pread(&mut self, path: &str, offset: u64, length: usize) -> VfsResult<Vec<u8>> {
        self.check_read_length(path, length)?;
        let file = self.open_read(path)?;
        let mut bytes = vec![0; length];
        let read = file
            .seek_read(&mut bytes, offset)
            .map_err(|error| io_error_to_vfs("pread", path, error))?;
        bytes.truncate(read);
        Ok(bytes)
    }

    fn pwrite(&mut self, path: &str, content: impl Into<Vec<u8>>, offset: u64) -> VfsResult<()> {
        // `std::os::windows::fs::FileExt::seek_write` uses an overlapped write,
        // but cap-std opens these capability handles for synchronous I/O. That
        // combination returns ERROR_ACCESS_DENIED. Kernel filesystem access is
        // serialized here, so an ordinary seek + write preserves pwrite's
        // guest-visible offset semantics without changing the descriptor's
        // independently tracked cursor.
        let mut file = self.open_write(path, false, false)?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| io_error_to_vfs("pwrite", path, error))?;
        file.write_all(&content.into())
            .map_err(|error| io_error_to_vfs("pwrite", path, error))
    }
}

#[derive(Clone)]
struct HostDirModuleMount {
    guest_prefix: String,
    filesystem: ModuleMountBackend,
}

#[derive(Clone)]
enum ModuleMountBackend {
    Host(HostDirFilesystem),
    Tar(TarFileSystem),
}

impl ModuleMountBackend {
    fn realpath(&self, path: &str) -> VfsResult<String> {
        match self {
            Self::Host(fs) => fs.realpath(path),
            Self::Tar(fs) => VirtualFileSystem::realpath(fs, path),
        }
    }

    fn read_file(&mut self, path: &str) -> VfsResult<Vec<u8>> {
        match self {
            Self::Host(fs) => fs.read_file(path),
            Self::Tar(fs) => VirtualFileSystem::read_file(fs, path),
        }
    }

    fn stat(&mut self, path: &str) -> VfsResult<VirtualStat> {
        match self {
            Self::Host(fs) => fs.stat(path),
            Self::Tar(fs) => VirtualFileSystem::stat(fs, path),
        }
    }

    fn exists(&self, path: &str) -> bool {
        match self {
            Self::Host(fs) => fs.exists(path),
            Self::Tar(fs) => VirtualFileSystem::exists(fs, path),
        }
    }
}

impl HostDirModuleMount {
    fn relative_virtual_path(&self, guest_path: &str) -> Option<String> {
        if guest_path == self.guest_prefix {
            return Some(String::from("/"));
        }
        let prefix = if self.guest_prefix == "/" {
            String::from("/")
        } else {
            format!("{}/", self.guest_prefix)
        };
        guest_path
            .strip_prefix(&prefix)
            .map(|relative| format!("/{relative}"))
    }

    fn guest_path_for_relative(&self, relative: &str) -> String {
        let relative = relative.trim_start_matches('/');
        if self.guest_prefix == "/" {
            format!("/{relative}")
        } else if relative.is_empty() {
            self.guest_prefix.clone()
        } else {
            format!("{}/{relative}", self.guest_prefix)
        }
    }
}

#[derive(Clone)]
pub(crate) struct HostDirModuleReader {
    mounts: Vec<HostDirModuleMount>,
}

impl HostDirModuleReader {
    pub(crate) fn from_mounts<I, G, H>(mounts: I) -> Option<Self>
    where
        I: IntoIterator<Item = (G, H)>,
        G: AsRef<str>,
        H: AsRef<Path>,
    {
        Self::from_mounts_and_package_tars(mounts, Vec::new())
    }

    pub(crate) fn from_mounts_and_package_tars<I, G, H>(
        mounts: I,
        package_tars: Vec<(String, String, String)>,
    ) -> Option<Self>
    where
        I: IntoIterator<Item = (G, H)>,
        G: AsRef<str>,
        H: AsRef<Path>,
    {
        let mut entries = mounts
            .into_iter()
            .filter_map(|(guest_path, host_path)| {
                Some(HostDirModuleMount {
                    guest_prefix: normalize_path(guest_path.as_ref()),
                    filesystem: ModuleMountBackend::Host(
                        HostDirFilesystem::new_with_read_limit(
                            host_path,
                            Some(MAX_HOST_DIR_READ_BYTES),
                        )
                        .ok()?,
                    ),
                })
            })
            .collect::<Vec<_>>();
        entries.extend(package_tars.into_iter().filter_map(|(guest, tar, root)| {
            Some(HostDirModuleMount {
                guest_prefix: normalize_path(&guest),
                filesystem: ModuleMountBackend::Tar(TarFileSystem::open_at(&tar, &root).ok()?),
            })
        }));
        if entries.is_empty() {
            return None;
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.guest_prefix.len()));
        entries.dedup_by(|left, right| left.guest_prefix == right.guest_prefix);
        Some(Self { mounts: entries })
    }

    fn mount_index_for(&self, guest_path: &str) -> Option<(usize, String)> {
        let normalized = normalize_path(guest_path);
        self.mounts.iter().enumerate().find_map(|(index, mount)| {
            mount
                .relative_virtual_path(&normalized)
                .map(|relative| (index, relative))
        })
    }
}

impl ModuleFsReader for HostDirModuleReader {
    fn canonical_guest_path(&mut self, guest_path: &str) -> Option<String> {
        let (index, relative) = self.mount_index_for(guest_path)?;
        let mount = &self.mounts[index];
        let resolved = mount.filesystem.realpath(&relative).ok()?;
        Some(mount.guest_path_for_relative(&resolved))
    }

    fn read_to_string(&mut self, guest_path: &str) -> Option<String> {
        let (index, relative) = self.mount_index_for(guest_path)?;
        String::from_utf8(self.mounts[index].filesystem.read_file(&relative).ok()?).ok()
    }

    fn path_is_dir(&mut self, guest_path: &str) -> Option<bool> {
        let (index, relative) = self.mount_index_for(guest_path)?;
        self.mounts[index]
            .filesystem
            .stat(&relative)
            .ok()
            .map(|stat| stat.is_directory)
    }

    fn path_exists(&mut self, guest_path: &str) -> bool {
        self.mount_index_for(guest_path)
            .is_some_and(|(index, relative)| self.mounts[index].filesystem.exists(&relative))
    }
}

pub(crate) struct SessionModuleReader {
    reader: HostDirModuleReader,
    cache: LocalModuleResolutionCache,
}

impl SessionModuleReader {
    pub(crate) fn new(reader: HostDirModuleReader) -> Self {
        Self {
            reader,
            cache: LocalModuleResolutionCache::default(),
        }
    }
}

impl GuestModuleReader for SessionModuleReader {
    fn read_module_source(&mut self, resolved_guest_path: &str) -> Option<String> {
        self.reader.read_to_string(resolved_guest_path)
    }

    fn resolve_module(&mut self, specifier: &str, referrer: &str) -> Option<String> {
        let reader: &mut dyn ModuleFsReader = &mut self.reader;
        ModuleResolver::new(reader, &mut self.cache).resolve_module(
            specifier,
            referrer,
            ModuleResolveMode::Import,
        )
    }
}

fn cap_time_parts(value: Option<cap_std::time::SystemTime>) -> (u64, u32) {
    let Some(value) = value else {
        return (0, 0);
    };
    let Ok(duration) = value.into_std().duration_since(UNIX_EPOCH) else {
        return (0, 0);
    };
    (
        duration.as_millis().min(u128::from(u64::MAX)) as u64,
        duration.subsec_nanos(),
    )
}

fn lexical_join(parent: &Path, target: &Path) -> VfsResult<PathBuf> {
    let mut result = parent.to_path_buf();
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => result.push(segment),
            Component::ParentDir => {
                if !result.pop() {
                    return Err(VfsError::access_denied(
                        "readlink",
                        &target.to_string_lossy(),
                        Some("symlink target escapes host directory"),
                    ));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(VfsError::access_denied(
                    "readlink",
                    &target.to_string_lossy(),
                    Some("symlink target escapes host directory"),
                ));
            }
        }
    }
    Ok(result)
}

fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from = from.components().collect::<Vec<_>>();
    let to = to.components().collect::<Vec<_>>();
    let shared = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = PathBuf::new();
    for _ in shared..from.len() {
        relative.push("..");
    }
    for component in &to[shared..] {
        relative.push(component.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative
    }
}

fn io_error_to_vfs(operation: &'static str, path: &str, error: io::Error) -> VfsError {
    let code = match error.kind() {
        io::ErrorKind::NotFound => "ENOENT",
        io::ErrorKind::PermissionDenied => "EACCES",
        io::ErrorKind::AlreadyExists => "EEXIST",
        io::ErrorKind::InvalidInput => "EINVAL",
        io::ErrorKind::IsADirectory => "EISDIR",
        io::ErrorKind::NotADirectory => "ENOTDIR",
        io::ErrorKind::DirectoryNotEmpty => "ENOTEMPTY",
        _ => "EIO",
    };
    VfsError::new(code, format!("{operation} '{path}': {error}"))
}
