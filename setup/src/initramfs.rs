use std::process::Command;

use ask;

pub fn rebuild() -> bool {
    println!();
    println!("Step 4: Update initramfs");
    let mut skip_ask = false;
    if ask::yesno("Are you using mkinitcpio with the default kernel ('linux')?") {
        let status = Command::new("/usr/bin/sudo").arg("/usr/bin/mkinitcpio")
            .arg("-p").arg("linux").status().expect("Failed to run mkinitcpio");
        if !status.success() {
            println!("Got an error from mkinitcpio. Sorry, but you have to fix this on your own.");
        } else {
            skip_ask = true;
        }
    } else {
        println!("Please run your initramfs generator now and verify that everything works.");
    }

    if !skip_ask {
        if !ask::yesno("Done?") {
            println!("Aborted.");
            return false;
        }
    }
    true
}
