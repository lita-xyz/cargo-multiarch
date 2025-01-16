use std::fs::File;
use std::io;

use libc::fexecve;
use proc_exit::{Code, Exit};
use rustix::fd::{IntoRawFd, OwnedFd, FromRawFd};
use rustix::fs::{memfd_create, MemfdFlags};

use super::{Executable, Binary};

/// A simple memfd + fexecve for fileless execution on Linux, BSDs and Solaris
/// See https://github.com/rust-lang/libc/pull/733/files for OS supported

impl Executable for Binary {
    fn create_writable(name: &str) -> Result<Self, io::Error> {
        // Note, there are permissions on the memory
        // and others on the file descriptor.
        // For example the EXEC flag is listed in Rustix:
        // - https://docs.rs/rustix/latest/rustix/fs/struct.MemfdFlags.html
        //   but it mentions kernel 6.3, is missing from BSDs,
        //   and even Linux docs: https://man7.org/linux/man-pages/man2/memfd_create.2.html
        // The file descriptor is writable by default.
        let file = memfd_create(name, MemfdFlags::CLOEXEC) // Close on exec
            .map(OwnedFd::into_raw_fd)
            .map(|fd| unsafe { File::from_raw_fd(fd)})?;
        Ok(Binary { file })
    }

    unsafe fn exec(
        self,
        _argc: i32,
        argv: *const *const i8,
        envp: *const *const i8,
    ) -> Result<(), Exit> {
        let status = unsafe { fexecve(self.file.into_raw_fd(), argv, envp) };
        Code::new(status).ok()
    }
}
