#![no_main]
#![no_std]
#![feature(offset_of)]

use core::panic::PanicInfo;
use core::time::Duration;
use wasabi::error;
use wasabi::executor::Executor;
use wasabi::executor::Task;
use wasabi::executor::TimeoutFuture;
use wasabi::hpet::global_timestamp;
use wasabi::info;
use wasabi::init::init_allocator;
use wasabi::init::init_basic_runtime;
use wasabi::init::init_display;
use wasabi::init::init_hpet;
use wasabi::init::init_paging;
use wasabi::print::hexdump;
use wasabi::print::set_global_vram;
use wasabi::println;
use wasabi::qemu::exit_qemu;
use wasabi::qemu::QemuExitCode;
use wasabi::uefi::init_vram;
use wasabi::uefi::locate_loaded_image_protocol;
use wasabi::uefi::EfiHandle;
use wasabi::uefi::EfiSystemTable;
use wasabi::warn;
use wasabi::x86::init_exceptions;

#[no_mangle]
fn efi_main(image_handle: EfiHandle, efi_system_table: &EfiSystemTable) {
    // Show initalized info
    println!("Booting WasabiOS...");
    println!("image_handle: {:#018X}", image_handle);
    println!("efi_system_table: {:#p}", efi_system_table);
    let loaded_image_protocol = locate_loaded_image_protocol(image_handle, efi_system_table)
        .expect("Failed to get LoadedImageProtocol");
    println!("image_base: {:#018X}", loaded_image_protocol.image_base);
    println!("image_size: {:#018X}", loaded_image_protocol.image_size);
    info!("info");
    warn!("warn");
    error!("error");
    hexdump(efi_system_table);

    // Initialize VRAM and test it
    let mut vram = init_vram(efi_system_table).expect("init_vram failed");
    init_display(&mut vram);

    // Get ACPI(Advanced Configuration and Power Interface)
    let acpi = efi_system_table
        .acpi_table()
        .expect("ACPI table not found.");

    // set global VRAM
    set_global_vram(vram);

    // memory map
    let memory_map = init_basic_runtime(image_handle, efi_system_table);

    // exit from efi boot services (Non-UEFI)
    info!("Entered Non-UEFI space!");

    // Initialize memory allocator
    init_allocator(&memory_map);

    // Exception handler test (with INT3-Breakpoint)
    let (_gdt, _idt) = init_exceptions();

    // Initialize page table from UEFI to original
    init_paging(&memory_map);
    info!("Now you are using your own page tables!");

    // Async
    init_hpet(acpi);
    let t0 = global_timestamp();

    let task1 = Task::new(async move {
        for i in 100..=103 {
            info!("{i} hpet.main_counter = {:?}", global_timestamp() - t0);
            // wait 1 sec
            TimeoutFuture::new(Duration::from_secs(1)).await;
        }
        Ok(())
    });
    let task2 = Task::new(async move {
        for i in 200..=203 {
            info!("{i} hpet.main_counter = {:?}", global_timestamp() - t0);
            // wait 2 sec
            TimeoutFuture::new(Duration::from_secs(2)).await;
        }
        Ok(())
    });

    let mut executor = Executor::new();

    executor.enqueue(task1);
    executor.enqueue(task2);
    Executor::run(executor)
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    error!("PANIC: {info:?}");
    exit_qemu(QemuExitCode::Fail)
}
