extern crate libudev;
extern crate num_cpus;
extern crate common;
extern crate driver;

pub mod iommu;
pub mod usb;
pub mod vfio;
pub mod initramfs;
pub mod vm;

use libudev::{Result as UdevResult, Context, Enumerator};
use common::pci_device::PciDevice;
use common::config::{Config, PciId};

pub fn get_vfio_bound_devs() -> UdevResult<Vec<PciId>> {
    let udev = Context::new().expect("Failed to create udev context");
    let mut iter = Enumerator::new(&udev)?;
    iter.match_property("DRIVER", "vfio-pci")?;

    Ok(iter.scan_devices()?.map(PciDevice::new).map(|x| x.id).collect())
}



pub struct Descriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub element: Element,
}

type Property<T> = &'static (Fn(&mut Config) -> &mut T + Sync);

pub enum Element {
    Boolean(Property<bool>),
    UInt(Property<usize>),
    Float(Property<f64>),
    String(Property<String>),

    OptString(Property<Option<String>>),

    Container {
        children: &'static [Descriptor],
        optionality: &'static Optionality,
    },
}

pub trait Optionality : Sync {
    fn exists(&self, cfg: &mut Config) -> bool;
    fn create(&self, cfg: &mut Config);
    fn delete(&self, cfg: &mut Config);
}

struct GenericOptionality<T, F: Fn(&mut Config) -> &mut Option<T> + Sync>(F);

impl<T: Default, F> Optionality for GenericOptionality<T, F>
    where F: Fn(&mut Config) -> &mut Option<T> + Sync {
    fn exists(&self, cfg: &mut Config) -> bool {
        self.0(cfg).is_some()
    }

    fn create(&self, cfg: &mut Config) {
        let old = ::std::mem::replace(self.0(cfg), Default::default());
        assert!(old.is_none());
    }

    fn delete(&self, cfg: &mut Config) {
        *self.0(cfg) = None;
    }
}

struct MandatoryOptionality;

impl Optionality for MandatoryOptionality {
    fn exists(&self, _: &mut Config) -> bool { true }
    fn create(&self, _: &mut Config) { unreachable!(); }
    fn delete(&self, _: &mut Config) { unreachable!(); }
}

pub static MANIFEST: &'static [Descriptor] = &[
    Descriptor {
        name: "machine",
        description: "",
        element: Element::Container {
            optionality: &MandatoryOptionality,
            children: &[
                Descriptor {
                    name: "memory",
                    description: "",
                    element: Element::String(&|cfg| &mut cfg.machine.memory),
                },
            ]
        }
    },
    Descriptor {
        name: "sound",
        description: "",
        element: Element::Container {
            optionality: &MandatoryOptionality,
            children: &[
                Descriptor {
                    name: "timer_period",
                    description: "",
                    element: Element::UInt(&|cfg| &mut cfg.sound.timer_period),
                },
            ]
        }
    },
    Descriptor {
        name: "samba",
        description: "",
        element: Element::Container {
            optionality: &GenericOptionality(|cfg| &mut cfg.samba),
            children: &[
            ]
        }
    },
    Descriptor {
        name: "additional_qemu_cmdline",
        description: "",
        element: Element::OptString(&|cfg| &mut cfg.additional_qemu_cmdline),
    },
    Descriptor {
        name: "data_directory_override",
        description: "",
        element: Element::OptString(&|cfg| &mut cfg.data_directory_override),
    },
];
