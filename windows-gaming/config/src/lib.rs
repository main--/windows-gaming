extern crate libudev;
extern crate num_cpus;
extern crate common;
extern crate enum_derive;

pub mod iommu;
pub mod usb;
pub mod vfio;
pub mod initramfs;
pub mod vm;

use libudev::{Result as UdevResult, Context, Enumerator};
use common::pci_device::PciDevice;
use common::config::{Config, PciId, SoundBackend, AlsaSettings};

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

pub struct EnumVariant {
    pub name: &'static str,
    pub matches: &'static (Fn(&Config) -> bool + Sync),
    pub set: &'static (Fn(&mut Config) + Sync),
    pub children: &'static [Descriptor],
}

type Property<T> = &'static (Fn(&mut Config) -> &mut T + Sync);

pub enum Element {
    Boolean(Property<bool>),
    UInt(Property<usize>),
    Float(Property<f64>),
    String(Property<String>),

    OptString(Property<Option<String>>),
    Enum(&'static FiniteStrings),

    Container {
        children: &'static [Descriptor],
        optionality: &'static Optionality,
    },

    ContainerEnum {
        variants: &'static [EnumVariant],
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

struct GenericFiniteStrings<T, F: Fn(&mut Config) -> &mut T + Sync>(F);

use std::fmt::Display;
use std::str::FromStr;
use enum_derive::IterVariantNames;

pub trait FiniteStrings : Sync {
    fn read(&self, cfg: &mut Config) -> String;
    fn write(&self, cfg: &mut Config, val: String) -> Result<(), ()>;
    fn enumerate(&self) -> Vec<&'static str>;
}

impl<T, F> FiniteStrings for GenericFiniteStrings<T, F>
    where T: Display + FromStr + IterVariantNames,
          F: Fn(&mut Config) -> &mut T + Sync {
    fn read(&self, cfg: &mut Config) -> String {
        self.0(cfg).to_string()
    }

    fn write(&self, cfg: &mut Config, val: String) -> Result<(), ()> {
        *self.0(cfg) = val.parse().map_err(|_| ())?;
        Ok(())
    }

    fn enumerate(&self) -> Vec<&'static str> {
        T::iter_variant_names().collect()
    }
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
                Descriptor {
                    name: "input",
                    description: "",
                    element: Element::Container {
                        optionality: &MandatoryOptionality,
                        children: &[
                            Descriptor {
                                name: "voices",
                                description: "",
                                element: Element::UInt(&|cfg| &mut cfg.sound.input.voices),
                            },
                            Descriptor {
                                name: "use_polling",
                                description: "",
                                element: Element::Boolean(&|cfg| &mut cfg.sound.input.use_polling),
                            },
                            Descriptor {
                                name: "fixed",
                                description: "",
                                element: Element::Container {
                                    optionality: &GenericOptionality(|cfg| &mut cfg.sound.input.fixed),
                                    children: &[
                                        Descriptor {
                                            name: "frequency",
                                            description: "",
                                            element: Element::UInt(&|cfg| &mut cfg.sound.input.fixed.as_mut().unwrap().frequency),
                                        },
                                        Descriptor {
                                            name: "format",
                                            description: "",
                                            element: Element::Enum(&GenericFiniteStrings(|cfg| &mut cfg.sound.input.fixed.as_mut().unwrap().format)),
                                        },
                                        Descriptor {
                                            name: "channels",
                                            description: "",
                                            element: Element::UInt(&|cfg| &mut cfg.sound.input.fixed.as_mut().unwrap().channels),
                                        },
                                    ]
                                }
                            },
                        ]
                    }
                },
                // TODO: copy-paste output
                Descriptor {
                    name: "backend",
                    description: "",
                    element: Element::ContainerEnum {
                        variants: &[
                            EnumVariant {
                                name: "None",
                                matches: &|cfg| match cfg.sound.backend { SoundBackend::None => true, _ => false },
                                set: &|cfg| cfg.sound.backend = SoundBackend::None,
                                children: &[],
                            },
                            EnumVariant {
                                name: "Alsa",
                                matches: &|cfg| match cfg.sound.backend { SoundBackend::Alsa { .. } => true, _ => false },
                                set: &|cfg| cfg.sound.backend = SoundBackend::Alsa {
                                    sink: AlsaSettings::default(),
                                    source: AlsaSettings::default(),
                                },
                                children: &[],
                            },
                            EnumVariant {
                                name: "PulseAudio",
                                matches: &|cfg| match cfg.sound.backend { SoundBackend::PulseAudio { .. } => true, _ => false },
                                set: &|cfg| cfg.sound.backend = SoundBackend::PulseAudio {
                                    buffer_samples: 4096,
                                    server: None,
                                    sink_name: None,
                                    source_name: None,
                                },
                                children: &[
                                    Descriptor {
                                        name: "buffer_samples",
                                        description: "",
                                        element: Element::UInt(&|cfg| match cfg.sound.backend {
                                            SoundBackend::PulseAudio { ref mut buffer_samples, .. } => buffer_samples,
                                            _ => unreachable!(),
                                        }),
                                    },
                                    Descriptor {
                                        name: "server",
                                        description: "",
                                        element: Element::OptString(&|cfg| match cfg.sound.backend {
                                            SoundBackend::PulseAudio { ref mut server, .. } => server,
                                            _ => unreachable!(),
                                        }),
                                    },
                                    Descriptor {
                                        name: "sink_name",
                                        description: "",
                                        element: Element::OptString(&|cfg| match cfg.sound.backend {
                                            SoundBackend::PulseAudio { ref mut sink_name, .. } => sink_name,
                                            _ => unreachable!(),
                                        }),
                                    },
                                    Descriptor {
                                        name: "source_name",
                                        description: "",
                                        element: Element::OptString(&|cfg| match cfg.sound.backend {
                                            SoundBackend::PulseAudio { ref mut source_name, .. } => source_name,
                                            _ => unreachable!(),
                                        }),
                                    },
                                ]
                            }
                        ]
                    }
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
