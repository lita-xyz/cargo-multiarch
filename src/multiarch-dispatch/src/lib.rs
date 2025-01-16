#![no_main]
#![feature(stdarch_internal)]
#![feature(associated_type_defaults)]

use libc::c_char;
use std::ffi::CStr;

use binary_flavors::{FatBin, Executable};
use proc_exit::{exit, Exit, sysexits::io_to_sysexists};

mod binary_flavors;

const FATBIN: FatBin<'static> = include_fatbin();

const fn include_fatbin<'a>() -> FatBin<'a> {
    include!(concat!(env!("OUT_DIR"), "/fatbin.rs"))
}

/// Entry point of the fat binary
/// This does
/// 1. CPU feature detection
/// 2. Creating the best optimized binary from the base one + patches
/// 3. Launch it, forwarding arguments and environment
///
/// This should be imported by the fat binary package that is auto-generated.
#[no_mangle]
pub unsafe extern "C" fn main(argc: i32, argv: *const *const c_char, envp: *const *const c_char) {
    let status = dispatch(argc, argv, envp);
    exit(status);
}

unsafe fn dispatch(
    argc: i32,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> Result<(), Exit> {
    let name_prefix = if argc > 0 {
        CStr::from_ptr(*argv).to_str().unwrap()
    } else {
        "unnamed_multiarch"
    };
    // Pretty sure the error can be handled in a simpler manner
    let bin = FATBIN.get_best_flavor(name_prefix).map_err(|e| io_to_sysexists(e.kind()).unwrap()).map_err(|code| code.as_exit())?;
    bin.exec(argc, argv, envp)
}
