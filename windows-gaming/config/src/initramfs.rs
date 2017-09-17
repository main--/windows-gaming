use std::process::Command;

pub fn run_mkinitcpio() -> bool {
    let status = Command::new("/usr/bin/sudo").arg("/usr/bin/mkinitcpio")
        .arg("-p").arg("linux").status().expect("Failed to run mkinitcpio");
    status.success()
}
