use std::path::PathBuf;

use clap;
use clap_cargo;

#[derive(clap::Parser)]
#[command(name = "cargo", bin_name = "cargo")]
pub enum Cargo {
    #[command(name = "multiarch", version, author, about, long_about)]
    Multiarch(Args),
}

/// Query RUSTC
#[derive(clap::ValueEnum, Clone, Copy)]
pub enum Print {
    /// List all (CPU, OS) "target triple" this version of rustc can build for.
    TargetList,
    /// List all CPUs available for "--target <TRIPLE>".
    /// Use --target-list to list available targets.
    /// Defaults to host TRIPLE.
    #[clap(verbatim_doc_comment)]
    TargetCpus,
    /// List CPU features supported by "--target-cpu".
    /// Use "--target <TRIPLE> --target-cpus" to list available CPUs for an architecture.
    /// Defaults to host CPU
    #[clap(verbatim_doc_comment)]
    TargetCpuFeatures,
}

#[derive(clap::Args)]
pub(crate) struct Args {
    /// Query or build for the target triple.
    /// For example "x86_64-unknown-linux-gnu" or "aarch64-apple-darwin".
    /// A target-triple is an LLVM concept.
    ///   <arch><sub>-<vendor>-<os>-<optionally abi/env>,
    /// unknown matches to any <vendor>
    /// See https://llvm.org/doxygen/Triple_8h_source.html
    #[clap(long, value_name = "TRIPLE", verbatim_doc_comment)]
    pub target: Option<String>,

    /// Query rustc
    #[clap(short, long, value_name = "QUERY")]
    pub print: Option<Print>,

    /// Query (query only) for the specified CPU
    #[clap(long, value_name = "CPU")]
    pub target_cpu: Option<String>,

    /// Copy final artifacts to this directory
    #[clap(short, long, value_name = "PATH")]
    pub out_dir: Option<PathBuf>,

    /// Build artifacts with the specified cargo profile
    /// Built-in profiles are dev, release, test, and bench
    #[clap(long, value_name = "PROFILE", default_value = "release")]
    pub profile: String,

    /// Comma-separated list of CPUs, a binary will be build for each.
    /// This overwrites Cargo.toml CPUs
    #[clap(
        long,
        use_value_delimiter = true,
        value_delimiter = ',',
        value_name = "CPUs"
    )]
    pub cpus: Option<Vec<String>>,

    /// A list of cpufeatures to support.
    /// When building from the CLI,
    /// it is not possible to set multiple cpufeatures based build
    /// due to clap limitation on Option<Vec<Vec<T>>> https://github.com/clap-rs/clap/issues/4626
    /// Use Cargo.toml instead.
    /// This overwrites Cargo.toml cpufeatures
    #[clap(
        short,
        long,
        use_value_delimiter = true,
        value_delimiter = ',',
        value_name = "CPUFEATURES"
    )]
    pub cpufeatures: Option<Vec<String>>,

    #[command(flatten)]
    pub manifest: clap_cargo::Manifest,

    #[command(flatten)]
    pub workspace: clap_cargo::Workspace,

    #[command(flatten)]
    pub features: clap_cargo::Features,

    /// Arguments given to cargo build
    #[clap(raw = true)]
    pub args: Vec<String>,
}
