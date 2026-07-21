#![cfg(windows)]

// Exercise the Windows implementation directly. The production module is
// crate-private, so this integration test includes the implementation just as
// the existing Unix host-dir tests do.
#[allow(dead_code)]
mod host_dir {
    include!("../src/plugins/host_dir_windows.rs");

    mod tests {
        use super::HostDirFilesystem;
        use agentos_kernel::vfs::VirtualFileSystem;
        use std::fs;
        use std::path::PathBuf;
        use std::time::{SystemTime, UNIX_EPOCH};

        fn temp_dir(prefix: &str) -> PathBuf {
            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be monotonic enough for temp paths")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("{prefix}-{suffix}"));
            fs::create_dir_all(&path).expect("create temp dir");
            path
        }

        #[test]
        fn filesystem_writes_files_at_mount_root() {
            let host_dir = temp_dir("agentos-windows-host-dir-root-write");
            let mut filesystem = HostDirFilesystem::new(&host_dir).expect("create host dir fs");

            filesystem
                .write_file("/root.txt", b"written from vm".to_vec())
                .expect("write file at mount root");

            assert_eq!(
                fs::read(host_dir.join("root.txt")).expect("read written host file"),
                b"written from vm"
            );
            fs::remove_dir_all(host_dir).expect("remove temp dir");
        }
    }
}
