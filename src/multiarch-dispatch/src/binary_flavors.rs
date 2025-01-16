use std::collections::HashSet;
use std::fs::File;
use std::io;

use notstd_detect::detect; // std::detect uses removed feature const_fn and no release since https://github.com/rust-lang/stdarch/issues/1526
use qbsdiff::Bspatch;
use zstd;
use cfg_if;
use proc_exit::Exit;

#[cfg(target_arch = "x86_64")]
mod features_x86;

cfg_if::cfg_if! {
if #[cfg(any(
    target_os = "android",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "linux",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "solaris"
))] {
        mod exec_memory;
    } else {
        mod exec_tempfile;
    }
  }

// Traits
// ---------------------------------------------------------------

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Copy)]
#[repr(transparent)] // transmute safe
pub(crate) struct CpuFeatList<'a>(&'a [&'a str]);

pub(crate) trait Features<'a> {
    fn get_features_lists(&'a self) -> &'a [CpuFeatList<'a>];
}
pub(crate) trait FlavorsRank<'a>: Features<'a> {

    /// Returns the index of the top ranked binary flavor
    /// The input should be a pre-filtered list of host CPU compatible features
    /// Returns -1 if empty
    fn get_top_ranked(supported_feat_lists: impl Iterator<Item = CpuFeatList<'a>>) -> isize;

    /// Filters the binaries that can run on this CPU
    /// and return a tuple of their original index and features
    fn get_supported_binaries(&'a self) -> (Vec<usize>, Vec<CpuFeatList<'a>>)
    {
        let host_features: HashSet<&str> = detect::features()
            .filter_map(|(name, is_available)| is_available.then_some(name))
            .collect();

        self.get_features_lists()
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, patch_feats)| {
                host_features.is_superset(
                    &HashSet::from_iter(patch_feats.0.iter().cloned())
                )
            })
            .unzip()
    }

    fn get_best_flavor_id(&'a self) -> Option<usize> {
        let (indices, feat_lists) = self.get_supported_binaries();
        if indices.len() == 0 {
            None
        } else {
            let top_compatible_index = Self::get_top_ranked(feat_lists.into_iter());
            // top_compatible_index != -1  due to the previous indices.len() == 0 check
            Some(indices[top_compatible_index as usize])
        }
    }
}
pub(crate) trait Executable: Sized {
    /// Create an executable in a temporary location
    /// and returns a handle to it.
    /// `name`` is indicative and used for debugging.
    /// The executable is created empty, pending code to execute
    /// The executable is run-once, consumed and dropped on success.
    fn create_writable(name: &str) -> Result<Self, io::Error>;

    /// Run the executable,
    /// it is consumed on success and file is closed.
    unsafe fn exec(
        self,
        argc: i32,
        argv: *const *const i8,
        envp: *const *const i8,
    ) -> Result<(), Exit>;
}

// Types
// ---------------------------------------------------------------

/// A fat binary type that contains a default executable
/// with no features
/// and may contain patches and a description of
/// corresponding CPU features
// This data structure should be kept as simple as possible
// to reduce compile-time.
// Furthermore, it should allow zero-copy views for memory efficiency.
pub(crate) struct FatBin<'a> {
    default_exe: &'a [u8],
    pub(crate) patches_features_lists: &'a [CpuFeatList<'a>],
    patches: &'a [&'a [u8]],
}

/// A binary unbundled from a fat binary
pub(crate) struct Binary {
    file: File,
}

// Impl
// ---------------------------------------------------------------

impl<'a> Features<'a> for FatBin<'a> {

    #[inline(always)]
    fn get_features_lists(&self) -> &[CpuFeatList<'_>] {
        self.patches_features_lists
    }
}

impl<'a> FatBin<'a> {
    fn extract_flavor_into(&self, mut output: impl io::Write, id: Option<usize>) -> io::Result<()> {
        // Prepare the binary flavor for execution,
        // Pass None for the default executable
        match id {
            None => zstd::stream::copy_decode(self.default_exe, &mut output),
            Some(id) => {
                let base = zstd::decode_all(self.default_exe)?;
                let patcher = Bspatch::new(self.patches[id])?;
                patcher.apply(&base, output)?;
                Ok(())
            }
        }
    }

    /// Load the best binary flavor
    /// `name_prefix` is used for debugging
    /// the flavor features will be appended to it.
    pub fn get_best_flavor(&'a self, name_prefix: &str) -> Result<Binary, io::Error>
    where
        Self: FlavorsRank<'a>,
        Binary: Executable,
    {
        let best_id = self.get_best_flavor_id();
        let suffix = if let Some(id) = best_id {
            self.patches_features_lists[id].0.join("_")
        } else {"generic".to_owned()};
        let bin_name = format!("{}_{}", name_prefix, &suffix);
        let mut bin: Binary = Executable::create_writable(&bin_name)?;
        self.extract_flavor_into(&mut bin.file, best_id)?;
        Ok(bin)
    }
}
