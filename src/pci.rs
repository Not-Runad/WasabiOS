use crate::acpi::AcpiMcfgDescriptor;
use crate::info;
use crate::result::Result;
use core::fmt;
use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::Range;
use core::ptr::read_volatile;

const MASK_BUS: usize = 0xff00;
const SHIFT_BUS: usize = 8;
const MASK_DEVICE: usize = 0x00f8;
const SHIFT_DEVICE: usize = 3;
const MASK_FUNCTION: usize = 0x000f;
const SHIFT_FUNCTION: usize = 0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VendorDeviceId {
    pub vendor: u16,
    pub device: u16,
}
impl VendorDeviceId {
    pub fn fmt_common(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "(vendor: {:#06X}, device: {:#06X})",
            self.vendor, self.device,
        )
    }
}
impl fmt::Debug for VendorDeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_common(f)
    }
}
impl fmt::Display for VendorDeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_common(f)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BusDeviceFunction {
    id: u16,
}
impl BusDeviceFunction {
    pub fn new(bus: usize, device: usize, function: usize) -> Result<Self> {
        if !(0..256).contains(&bus) || !(0..32).contains(&device) || !(0..8).contains(&function) {
            Err("PCI bus device function out of range.")
        } else {
            Ok(Self {
                id: ((bus << SHIFT_BUS) | (device << SHIFT_DEVICE) | (function << SHIFT_FUNCTION))
                    as u16,
            })
        }
    }

    pub fn bus(&self) -> usize {
        ((self.id as usize) & MASK_BUS) >> SHIFT_BUS
    }

    pub fn device(&self) -> usize {
        ((self.id as usize) & MASK_DEVICE) >> SHIFT_DEVICE
    }

    pub fn function(&self) -> usize {
        ((self.id as usize) & MASK_FUNCTION) >> SHIFT_FUNCTION
    }

    pub fn iter() -> BusDeviceFunctionIterator {
        BusDeviceFunctionIterator { next_id: 0 }
    }

    pub fn fmt_common(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "/pci/bus/{:#04X}/device/{:#04X}/function/{:#03X})",
            self.bus(),
            self.device(),
            self.function(),
        )
    }
}
impl fmt::Debug for BusDeviceFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_common(f)
    }
}
impl fmt::Display for BusDeviceFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_common(f)
    }
}

pub struct BusDeviceFunctionIterator {
    next_id: usize,
}
impl Iterator for BusDeviceFunctionIterator {
    type Item = BusDeviceFunction;
    fn next(&mut self) -> Option<Self::Item> {
        let id = self.next_id;
        if id > 0xffff {
            None
        } else {
            self.next_id += 1;
            let id = id as u16;
            Some(BusDeviceFunction { id })
        }
    }
}

struct ConfigRegisters<T> {
    access_type: PhantomData<T>,
}
impl<T> ConfigRegisters<T> {
    fn read(ecm_base: *mut T, byte_offset: usize) -> Result<T> {
        if !(0..256).contains(&byte_offset) || byte_offset % size_of::<T>() != 0 {
            Err("PCI ConfigRegisters read out of range.")
        } else {
            Ok(unsafe { read_volatile(ecm_base.add(byte_offset / size_of::<T>())) })
        }
    }
}

pub struct Pci {
    ecm_range: Range<usize>,
}
impl Pci {
    pub fn new(mcfg: &AcpiMcfgDescriptor) -> Self {
        // To simplify, assume that there is one mcfg entry that maps all the cpi configuration spaces.
        assert!(mcfg.num_of_entries() == 1);
        let pci_config_space_base = mcfg.entry(0).expect("Out of range.").base_address() as usize;
        let pci_config_space_end = pci_config_space_base + (1 << 24);
        Self {
            ecm_range: pci_config_space_base..pci_config_space_end,
        }
    }

    pub fn ecm_base<T>(&self, bus_device_function: BusDeviceFunction) -> *mut T {
        (self.ecm_range.start + ((bus_device_function.id as usize) << 12)) as *mut T
    }

    pub fn read_register_u16(
        &self,
        bus_device_function: BusDeviceFunction,
        byte_offset: usize,
    ) -> Result<u16> {
        ConfigRegisters::read(self.ecm_base(bus_device_function), byte_offset)
    }

    pub fn read_vendor_id_and_device_id(
        &self,
        bus_device_function: BusDeviceFunction,
    ) -> Option<VendorDeviceId> {
        let vendor = self.read_register_u16(bus_device_function, 0).ok()?;
        let device = self.read_register_u16(bus_device_function, 2).ok()?;
        if vendor == 0xffff || device == 0xffff {
            // Not connected
            None
        } else {
            Some(VendorDeviceId { vendor, device })
        }
    }

    pub fn probe_devices(&self) {
        for bus_device_function in BusDeviceFunction::iter() {
            if let Some(vdid) = self.read_vendor_id_and_device_id(bus_device_function) {
                info!("{vdid}");
            }
        }
    }
}
