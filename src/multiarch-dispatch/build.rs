//! Build script that generates a Rust file that contains a compressed source binary and a set of compressed patches for each CPU features set.
//!
//! It reads a JSON file that contains a set of paths to executables and their dependency on CPU features
//! from the environment variable `MULTIARCH_ARTIFACTS`.
//! Then, it generates a Rust file that contains the source and the patches.
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use proc_exit::sysexits::io_to_sysexists;
use zstd;
use qbsdiff::Bsdiff;
use quote::quote;
use serde::Deserialize;
use proc_exit::Exit;

#[derive(Default, Deserialize)]
struct BinaryDesc {
    path: PathBuf,
    // Empty for the default fallback binary
    cpufeatures: Vec<String>,
}

#[derive(Default, Deserialize)]
struct Artifacts {
    bins: Vec<BinaryDesc>,
}

fn bsdiff(source: &[u8], target: &[u8]) -> Result<Vec<u8>, Exit> {
    let mut patch = Vec::new();
    Bsdiff::new(source, target)
        .compare(std::io::Cursor::new(&mut patch))
        .map_err(|_| proc_exit::sysexits::IO_ERR.with_message("Failed to generate a patch"))?;
    Ok(patch)
}

impl Artifacts {
    fn from_env() -> Option<Result<Self, Exit>> {
        let path = option_env!("MULTIARCH_ARTIFACTS")?;

        println!("cargo:rerun-if-env-changed=MULTIARCH_ARTIFACTS");
        println!("cargo:rerun-if-changed={path}");

        Some(Self::from_path(path))
    }

    fn from_path(path: impl AsRef<Path>) -> Result<Self, Exit> {
        let path = path.as_ref();
        let file = File::open(path).map_err(|_| {
            proc_exit::sysexits::IO_ERR.with_message(format!(
                "Failed to open the build artifacts file {}",
                path.display()
            ))
        })?;
        let mut bins: Self =
            serde_json::from_reader(BufReader::new(file)).map_err(|_| {
                proc_exit::sysexits::DATA_ERR.with_message(format!(
                    "Failed to parse the artifacts description file {}",
                    path.display(),
                ))
            })?;

        bins.sort_by_features();
        bins.print_rerun();

        Ok( bins )
    }

    /// Sort the builds to put the ones requiring more features at the head
    fn sort_by_features(&mut self) {
        self.bins.sort_unstable_by(|build1, build2| {
            build1.cpufeatures.len().cmp(&build2.cpufeatures.len()).reverse()
        });
    }

    fn print_rerun(&self) {
        let mut stdout = std::io::stdout().lock();
        for bin in &self.bins {
            let _ = writeln!(stdout, "cargo:rerun-if-changed={}", bin.path.display());
        }
    }

    pub fn generate_sources(mut self, dest_path: &Path) -> Result<(), Exit> {
        let fallback_desc = self.bins.pop(); // Binaries are sorted, the one with no features is the fallback

        if fallback_desc.is_none() {
            println!("cargo:warning=The JSON file loaded from the environment variable MULTIARCH_ARTIFACTS is empty.");
        }

        let fallback = fallback_desc
            .as_ref()
            .map(|fallback| {
                std::fs::read(&fallback.path).map_err(|_| {
                    proc_exit::sysexits::IO_ERR.with_message(format!(
                        "Failed to read fallback build {}",
                        fallback.path.display(),
                    ))
                })
            })
            .transpose()?
            .unwrap_or_default();

        let (patches, features_lists): (Vec<_>, Vec<_>) = self
            .bins
            .into_iter()
            .map(|bin| {
                let target = std::fs::read(&bin.path).map_err(|_| {
                    proc_exit::sysexits::IO_ERR
                        .with_message(format!("Failed to read binary {}", bin.path.display(),))
                }).unwrap(); // TODO: fix the error bubble up
                let patch = bsdiff(&fallback, &target).unwrap(); // TODO: fix the error bubble up
                let features = bin.cpufeatures;
                let patch_raw = quote! {&[#(#patch),*]};
                let features_raw = quote! {&[#(#features),*]};
                (patch_raw, features_raw)
            })
            .unzip();
        let source = zstd::stream::encode_all(&fallback[..], 3).map_err(|e| io_to_sysexists(e.kind()).unwrap()).map_err(|code| code.as_exit())?;

        let source = &source;
        let features_lists = &features_lists;
        let patches = &patches;

        let fatbin_raw = quote! {
            FatBin {
                default_exe: &[#(#source),*],
                patches_features_lists: &[#(#features_lists),*],
                patches: &[#(#patches),*],
            };
        };

        std::fs::write(dest_path, fatbin_raw.to_string()).map_err(|_| {
            proc_exit::sysexits::IO_ERR.with_message(format!(
                "Failed to write generated Rust file to {}",
                dest_path.display(),
            ))
        })?;

        Ok(())
    }
}

fn main() -> Result<(), Exit> {
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = std::env::var_os("OUT_DIR").ok_or_else(|| {
        proc_exit::sysexits::SOFTWARE_ERR.with_message("Missing OUT_DIR environment variable")
    })?;
    let raw_fatbin = Path::new(&out_dir).join("fatbin.rs");

    let artifacts = Artifacts::from_env()
        .transpose()?
        .unwrap_or_default();

        artifacts.generate_sources(&raw_fatbin)?;

    Ok(())
}
