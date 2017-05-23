//! The `setup` mod is used for setting up the system to achieve GPU
//! passthrough.
//! A reboot is definitely needed, but we should only force one repoot.
//! Therefore the setup is split into two phases:
//! 1. Pre-Reboot phase: Here everything should be done which is not bound to
//!    a reboot.
//!    The more we can do before the reboot the better.
//! 2. Post-Reboot phase: Everything that is depended on the reboot should be
//!    done here.
//!    If possible, it should be prepared as well as possible in the pre-reboot
//!    phase.

mod ask;
mod iommu;
mod usb;
mod vfio;
mod gpu;
mod initramfs;
mod vm;
mod wizard;

pub use self::wizard::run;
