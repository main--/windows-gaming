use std::path::Path;
use std::fmt::Write as FmtWrite;

use common::config::SambaConfig;

pub fn is_installed() -> bool {
    Path::new("/usr/sbin/smbd").is_file()
}

pub fn setup(_tmp: &Path, samba: &SambaConfig, usernet: &mut String) {
    assert!(is_installed(), "Optional samba dependency not installed!");

    write!(usernet, ",smb={}", samba.path).unwrap();
}
