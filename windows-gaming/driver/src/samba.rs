use std::fs::{create_dir, File};
use std::path::Path;
use std::io::Write;
use std::fmt::Write as FmtWrite;

use common::config::SambaConfig;

pub fn is_installed() -> bool {
    Path::new("/usr/sbin/smbd").is_file()
}

pub fn setup(tmp: &Path, samba: &SambaConfig, usernet: &mut String) {
    assert!(is_installed(), "Optional samba dependency not installed!");

    let samba_cfg = tmp.join("smbd.conf");
    let mut smbd_conf = File::create(&samba_cfg).expect("Failed to create smbd conf");
    let samba_folder = tmp.join("samba");
    write!(smbd_conf,
           r#"
[global]
private dir={0}
interfaces=127.0.0.1
bind interfaces only=yes
pid directory={0}
lock directory={0}
state directory={0}
cache directory={0}
ncalrpc dir={0}/ncalrpc
log file={0}/log.smbd
smb passwd file={0}/smbpasswd
security = user
map to guest = Bad User
load printers = no
printing = bsd
disable spoolss = yes
usershare max shares = 0
create mask = 0644
[qemu]
path={1}
read only=no
guest ok=yes
force user={2}
"#,
           samba_folder.display(),
           samba.path,
           samba.user)
        .expect("Failed to write smbd conf");

    create_dir(&samba_folder).expect("Failed to create samba folder");
    /*
    TODO: running samba as a different user only made sense in root mode. Plus we need to rework all of this anyway.
    chown(&samba_folder,
          Some(user.uid()),
          Some(user.primary_group_id()))
        .expect("Failed to chown samba folder");
    */
    write!(usernet,
           ",guestfwd=tcp:10.0.2.1:445-cmd:sudo -u {} -- smbd --configfile {}",
           samba.user,
           samba_cfg.display())
        .unwrap();
}
