use crate::acpi::AcpiMcfgDescriptor;
use crate::error;
use crate::info;
use crate::result::Result;
use crate::x86::with_current_page_table;
use crate::x86::PageAttr;
use crate::xhci::PciXhciDriver;
use core::fmt;
use core::marker::PhantomData;
use core::mem::size_of;
use core::ops::Range;
use core::ptr::read_volatile;
use core::ptr::write_volatile;

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

    fn write(ecm_base: *mut T, byte_offset: usize, data: T) -> Result<()> {
        if !(0..256).contains(&byte_offset) || byte_offset % size_of::<T>() != 0 {
            Err("PCI ConfigRegisters write out of range.")
        } else {
            unsafe {
                write_volatile(ecm_base.add(byte_offset / size_of::<T>()), data);
            }
            Ok(())
        }
    }
}

pub struct BarMem64 {
    addr: *mut u8,
    size: u64,
}
impl BarMem64 {
    pub fn addr(&self) -> *mut u8 {
        self.addr
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn disable_cache(&self) {
        let vstart = self.addr() as u64;
        let vend = self.addr() as u64 + self.size();
        unsafe {
            with_current_page_table(|pt| {
                pt.create_mapping(vstart, vend, vstart, PageAttr::ReadWriteIo)
                    .expect("Failed to create mapping.")
            })
        }
    }
}
impl fmt::Debug for BarMem64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BarMem64[{:#018X}..{:#018X}]",
            self.addr as u64,
            self.addr as u64 + self.size()
        )
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

    pub fn read_vendor_device_id(
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
            if let Some(vendor_device_id) = self.read_vendor_device_id(bus_device_function) {
                info!("{vendor_device_id}");
                if PciXhciDriver::supports(vendor_device_id) {
                    if let Err(e) = PciXhciDriver::attach(self, bus_device_function) {
                        error!("PCI: driver attach() failed: {e:?}")
                    } else {
                        continue;
                    }
                }
            }
        }
    }

    pub fn read_register_u32(
        &self,
        bus_device_function: BusDeviceFunction,
        byte_offset: usize,
    ) -> Result<u32> {
        ConfigRegisters::read(self.ecm_base(bus_device_function), byte_offset)
    }

    pub fn write_register_u32(
        &self,
        bus_device_function: BusDeviceFunction,
        byte_offset: usize,
        data: u32,
    ) -> Result<()> {
        ConfigRegisters::write(self.ecm_base(bus_device_function), byte_offset, data)
    }

    pub fn read_register_u64(
        &self,
        bus_device_function: BusDeviceFunction,
        byte_offset: usize,
    ) -> Result<u64> {
        let lo = self.read_register_u32(bus_device_function, byte_offset)?;
        let hi = self.read_register_u32(bus_device_function, byte_offset + 4)?;
        Ok(((hi as u64) << 32) | (lo as u64))
    }

    pub fn write_register_u64(
        &self,
        bus_device_function: BusDeviceFunction,
        byte_offset: usize,
        data: u64,
    ) -> Result<()> {
        let lo: u32 = data as u32;
        let hi: u32 = (data >> 32) as u32;
        self.write_register_u32(bus_device_function, byte_offset, lo)?;
        self.write_register_u32(bus_device_function, byte_offset + 4, hi)?;
        Ok(())
    }

    pub fn try_bar0_mem64(&self, bus_device_function: BusDeviceFunction) -> Result<BarMem64> {
        let bar0 = self.read_register_u64(bus_device_function, 0x10)?;
        if bar0 & 0b0111 == 0b0100 {
            // Memory, 64bit, and Non-prefetchable
            let addr = (bar0 & !0b1111) as *mut u8;
            // Write all-1s to get the size of the region
            self.write_register_u64(bus_device_function, 0x10, !0_u64)?;
            let size = 1 + !(self.read_register_u64(bus_device_function, 0x10)? & !0b1111);
            // Restore the original value
            self.write_register_u64(bus_device_function, 0x10, bar0)?;
            Ok(BarMem64 { addr, size })
        } else {
            Err("Unexpected BAR0 type.")
        }
    }

    pub fn set_command_and_status_flags(
        &self,
        bus_device_function: BusDeviceFunction,
        flags: u32,
    ) -> Result<()> {
        let cmd_and_status =
            self.read_register_u32(bus_device_function, 0x04 /* Command and status */)?;
        self.write_register_u32(
            bus_device_function,
            0x04, /* Command and status */
            flags | cmd_and_status,
        )
    }

    pub fn enable_bus_master(&self, bus_device_function: BusDeviceFunction) -> Result<()> {
        self.set_command_and_status_flags(bus_device_function, 1 << 2 /* Bus Master enable */)
    }

    pub fn disable_interrupt(&self, bus_device_function: BusDeviceFunction) -> Result<()> {
        self.set_command_and_status_flags(bus_device_function, 1 << 10 /* Interrupt disable */)
    }
}
