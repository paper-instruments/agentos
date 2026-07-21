use crate::common::stable_hash64;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const NODE_COMPILE_CACHE_ENV: &str = "NODE_COMPILE_CACHE";
pub(crate) const NODE_DISABLE_COMPILE_CACHE_ENV: &str = "NODE_DISABLE_COMPILE_CACHE";
pub(crate) const NODE_FROZEN_TIME_ENV: &str = "AGENTOS_FROZEN_TIME_MS";
pub(crate) const NODE_SANDBOX_ROOT_ENV: &str = "AGENTOS_SANDBOX_ROOT";

pub(crate) fn env_flag_enabled(env: &BTreeMap<String, String>, key: &str) -> bool {
    env.get(key).is_some_and(|value| value == "1")
}

pub(crate) fn resolve_execution_path(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

pub(crate) fn warmup_marker_path(
    marker_dir: &Path,
    prefix: &str,
    version: &str,
    contents: &str,
) -> PathBuf {
    marker_dir.join(format!(
        "{prefix}-v{version}-{:016x}.stamp",
        stable_hash64(contents.as_bytes())
    ))
}

pub(crate) fn file_fingerprint(path: &Path) -> String {
    match fs::metadata(path) {
        #[cfg(unix)]
        Ok(metadata) => format!(
            "{}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.size(),
            metadata.mtime(),
            metadata.mtime_nsec(),
        ),
        #[cfg(windows)]
        Ok(metadata) => format!(
            "{}:{}:{}:{}",
            metadata.len(),
            system_time_nanos(metadata.modified().ok()),
            metadata.permissions().readonly(),
            metadata.is_dir(),
        ),
        Err(_) => String::from("missing"),
    }
}

#[cfg(unix)]
pub(crate) fn host_stat_value(metadata: &fs::Metadata) -> Value {
    json!({
        "mode": metadata.mode(),
        "size": metadata.size(),
        "blocks": metadata.blocks(),
        "dev": metadata.dev(),
        "rdev": metadata.rdev(),
        "isDirectory": metadata.is_dir(),
        "isSymbolicLink": metadata.file_type().is_symlink(),
        "atimeMs": metadata.atime() * 1000 + (metadata.atime_nsec() / 1_000_000),
        "mtimeMs": metadata.mtime() * 1000 + (metadata.mtime_nsec() / 1_000_000),
        "ctimeMs": metadata.ctime() * 1000 + (metadata.ctime_nsec() / 1_000_000),
        "birthtimeMs": metadata.ctime() * 1000 + (metadata.ctime_nsec() / 1_000_000),
        "ino": metadata.ino(),
        "nlink": metadata.nlink(),
        "uid": metadata.uid(),
        "gid": metadata.gid(),
    })
}

#[cfg(windows)]
pub(crate) fn host_stat_value(metadata: &fs::Metadata) -> Value {
    // The guest consumes POSIX-shaped metadata even though the backing store is
    // NTFS. These compatibility values never participate in path authorization;
    // the filesystem capability layer remains authoritative.
    let file_type = metadata.file_type();
    let mode_type = if file_type.is_symlink() {
        0o120000
    } else if metadata.is_dir() {
        0o040000
    } else {
        0o100000
    };
    let permissions = if file_type.is_symlink() {
        0o777
    } else if metadata.permissions().readonly() {
        0o444
    } else if metadata.is_dir() {
        0o755
    } else {
        0o666
    };
    let mtime_ms = system_time_millis(metadata.modified().ok());
    json!({
        "mode": mode_type | permissions,
        "size": metadata.len(),
        "blocks": metadata.len().div_ceil(512),
        "dev": 0,
        "rdev": 0,
        "isDirectory": metadata.is_dir(),
        "isSymbolicLink": file_type.is_symlink(),
        "atimeMs": system_time_millis(metadata.accessed().ok()),
        "mtimeMs": mtime_ms,
        "ctimeMs": mtime_ms,
        "birthtimeMs": system_time_millis(metadata.created().ok()),
        "ino": 0,
        "nlink": 1,
        "uid": 0,
        "gid": 0,
    })
}

#[cfg(windows)]
fn system_time_millis(value: Option<SystemTime>) -> u64 {
    value
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(windows)]
fn system_time_nanos(value: Option<SystemTime>) -> u128 {
    value
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::file_fingerprint;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn file_fingerprint_tracks_inode_and_mutation_time() {
        let temp = tempdir().expect("create temp dir");
        let path = temp.path().join("module.wasm");

        fs::write(&path, b"first").expect("write wasm file");
        let metadata = fs::metadata(&path).expect("stat wasm file");
        let first = file_fingerprint(&path);

        assert_eq!(
            first,
            format!(
                "{}:{}:{}:{}:{}",
                metadata.dev(),
                metadata.ino(),
                metadata.size(),
                metadata.mtime(),
                metadata.mtime_nsec(),
            )
        );

        std::thread::sleep(std::time::Duration::from_millis(25));
        fs::write(&path, b"second").expect("overwrite wasm file");

        assert_ne!(
            file_fingerprint(&path),
            first,
            "rewriting a tracked asset in place must invalidate warmup markers"
        );
    }

    #[cfg(windows)]
    #[test]
    fn file_fingerprint_tracks_mutation_time() {
        let temp = tempdir().expect("create temp dir");
        let path = temp.path().join("module.wasm");
        fs::write(&path, b"first").expect("write wasm file");
        let first = file_fingerprint(&path);

        std::thread::sleep(std::time::Duration::from_millis(25));
        fs::write(&path, b"second").expect("overwrite wasm file");

        assert_ne!(file_fingerprint(&path), first);
    }
}
