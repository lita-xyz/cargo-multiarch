use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use anyhow::{anyhow, Context};
use cargo_metadata::{Metadata, Package};
use clap_cargo;
use console::{style, Term};
use escargot::CargoBuild;
use indicatif::{ProgressBar, ProgressStyle};
use itertools::Itertools;
use serde::Serialize;
use sha2::{Digest, Sha256};
use target_lexicon::{Environment, Triple};

use crate::cargo_config_loader::{ConfigMultiArch, CpuFeatures};
use crate::cargo_msg_parser::CommandMessagesExt;
use crate::cli::Args;
use crate::gen_fatbin_pkg::FatbinCrate;
use crate::rustc_queries::Rustc;

#[derive(Serialize)]
struct BinaryDesc {
    path: PathBuf,
    // Empty for the default fallback binary
    cpufeatures: Vec<String>,
    #[serde(skip)]
    original_filename: Option<OsString>,
}

#[derive(Default, Serialize)]
struct Artifacts {
    bins: Vec<BinaryDesc>,
}
pub(crate) struct Multiarch {
    metadata: Metadata,
    target: Triple,      // CPU target
    target_dir: PathBuf, // Rust compilation /target directory
    outdir: Option<PathBuf>,
    fatbin: FatbinCrate,
    workspace: clap_cargo::Workspace,
    pkg_features: clap_cargo::Features, // passed to cargo as --features <list> like --features derive
    override_cpus: BTreeSet<String>,
    override_cpufeatures: CpuFeatures,
    progress: ProgressBar,
    profile: String,
    profile_dir: String,
    cargo_args: Vec<String>,
}

impl Multiarch {
    pub(crate) fn from_args(args: Args) -> anyhow::Result<Self> {
        let metadata = args
            .manifest
            .metadata()
            .exec()
            .context("Failed to execute `cargo metadata`")?;

        let target = Rustc::target_triple_or_host(args.target.as_deref()).and_then(|triple| {
            Triple::from_str(&triple)
                .map_err(|e| anyhow!("Error while parsing target triple '{triple}': {e}"))
        })?;
        let override_cpus: BTreeSet<String> =
            args.cpus.iter().flat_map(ToOwned::to_owned).collect();
        let override_cpufeatures: CpuFeatures = args
            .cpufeatures
            .iter()
            .flat_map(ToOwned::to_owned)
            .collect();

        // Rust <project root>/target
        let target_dir = metadata
            .target_directory
            .join(clap::crate_name!())
            .into_std_path_buf();

        let fatbin = FatbinCrate::generate(target_dir.clone())?;

        let progress = indicatif::ProgressBar::new(0).with_style(
            ProgressStyle::with_template(
                "{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} {spinner}",
            )?
            .progress_chars("=> "),
        );
        progress.enable_steady_tick(Duration::from_millis(200));

        let profile_dir = if args.profile == "dev" {
            "debug"
        } else {
            &args.profile
        }
        .to_owned();

        Ok(Self {
            metadata,
            target,
            target_dir,
            outdir: args.out_dir,
            fatbin,
            workspace: args.workspace,
            pkg_features: args.features,
            override_cpus,
            override_cpufeatures,
            progress,
            cargo_args: args.args,
            profile: args.profile,
            profile_dir,
        })
    }

    pub fn compile_workspace(&self) -> anyhow::Result<()> {
        let (pkgs, _) = self.workspace.partition_packages(&self.metadata);

        if !pkgs
            .iter()
            .any(|&package| package.targets.iter().any(|target| target.is_bin()))
        {
            anyhow::bail!("cargo-multiarch can only build binaries.");
        }

        for pkg in pkgs {
            println!(
                "{:>12} {} v{} ({})",
                style("Compiling").bold().green(),
                pkg.name,
                pkg.version,
                self.metadata.workspace_root
            );

            let pkg_multiarch = self.compile_pkg_multi(pkg)?;

            let original_filename = pkg_multiarch.bins
                .iter()
                .find_map(|pkg_arch| pkg_arch.original_filename.clone())
                .unwrap_or_else(|| {
                    format!("multiarch-placeholder{}", std::env::consts::EXE_SUFFIX).into()
                });

            if let [build] = &pkg_multiarch.bins[..] {
                self.handle_single_arch(build, original_filename)?
            } else {
                self.handle_multi_arch(&pkg_multiarch, original_filename, &pkg.name)?
            }
        }
        Ok(())
    }

    fn handle_single_arch(
        &self,
        build: &BinaryDesc,
        original_filename: OsString,
    ) -> anyhow::Result<()> {
        let output_path = self
            .target_dir
            .join(&self.target.to_string())
            .join(&self.profile_dir)
            .join(&original_filename);

        fs::rename(&build.path, &output_path).with_context(|| {
            format!(
                "Failed to rename `{}` to `{}`",
                build.path.display(),
                output_path.display()
            )
        })?;

        if let Some(out_dir) = self.outdir.as_deref() {
            fs::create_dir_all(out_dir).with_context(|| {
                format!("Failed to create output directory `{}`", out_dir.display())
            })?;
            let to = out_dir.join(&original_filename);
            fs::copy(&output_path, &to).with_context(|| {
                format!(
                    "Failed to copy `{}` to `{}`",
                    output_path.display(),
                    to.display()
                )
            })?;
        }

        println!(
            "{:>12} 1 version, no dispatcher needed ({})",
            style("Finished").bold().green(),
            output_path.display()
        );

        Ok(())
    }

    fn handle_multi_arch(
        &self,
        artifacts: &Artifacts,
        original_filename: OsString,
        pkg_name: &str,
    ) -> anyhow::Result<()> {
        let serialized =
            serde_json::to_vec_pretty(artifacts).context("Failed to encode the builds")?;

        let pkg_outdir = self.target_dir.join(pkg_name);
        fs::create_dir_all(&pkg_outdir).context("Failed to create temporary output directory")?;

        let artifacts_json = pkg_outdir.join("multiarch-artifacts.json");
        std::fs::write(&artifacts_json, serialized)
            .with_context(|| format!("Failed to write to `{}`", artifacts_json.display()))?;

        println!(
            "{:>12} {} versions packed into a fat binary",
            style("Compiling").bold().green(),
            artifacts.bins.len(),
        );

        let fatbin_path =
            self.fatbin
                .cargo_build(&self.target.to_string(), &artifacts_json, &original_filename)?;

        if let Some(out_dir) = self.outdir.as_deref() {
            std::fs::create_dir_all(out_dir).with_context(|| {
                format!("Failed to create output directory `{}`", out_dir.display())
            })?;
            let to = out_dir.join(&original_filename);
            std::fs::copy(&fatbin_path, &to).with_context(|| {
                format!(
                    "Failed to copy `{}` to `{}`",
                    fatbin_path.display(),
                    to.display()
                )
            })?;
        }

        println!(
            "{:>12} ({})",
            style("Finished").bold().green(),
            fatbin_path.display()
        );

        Ok(())
    }

    /// Compile a single package from the workspace
    /// for a multiset of CPU features
    fn compile_pkg_multi(&self, package: &Package) -> anyhow::Result<Artifacts> {
        let cargo_toml = package.manifest_path.as_std_path();
        let pkg_features = self.pkg_features.features.join(" ");
        let mut rust_flags = std::env::var("RUSTFLAGS").unwrap_or_default();

        let cargo_config = ConfigMultiArch::new(self.target.clone())
            .load_cargo_toml(package)
            .and_then(|cfg| cfg.override_cpus(self.override_cpus.clone()))
            .and_then(|cfg| {
                cfg.override_features_lists(BTreeSet::from([self.override_cpufeatures.clone()]))
            })?;

        let cpu_features = cargo_config.get_cpu_features();
        println!("CpuFeatures: {:?}", cpu_features);
        if cpu_features.is_empty() {
            anyhow::bail!(
                "No CPU arch or CPU features configured in CLI or in Cargo.toml's [package.metadata.multiarch.<CPU ARCH>]"
            );
        }

        self.progress.set_length(cpu_features.len() as u64);
        self.progress.set_prefix("Building");

        if self.target.environment == Environment::Msvc {
            rust_flags.push_str(" -C link-args=/Brepro");
        };

        let mut binaries_desc: Vec<([u8; 32], BinaryDesc)> = Default::default();

        self.progress.disable_steady_tick();
        self.progress.set_style(
            ProgressStyle::with_template(if Term::stdout().size().1 > 80 {
                "{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} (time remaining {eta}) {wide_msg}"
            } else {
                "{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len}"
            })?
            .progress_chars("=> "),
        );

        // Because we append CpuFeatures to an empty set, the first build is always the default one.
        for current_feature_set in cpu_features.iter() {
            let desc =
                self.compile_pkg(cargo_toml, &rust_flags, &pkg_features, current_feature_set)?;
            binaries_desc.push(desc);
        }

        binaries_desc.sort_unstable_by(|(h1, b1), (h2, b2)| {
            // First, we sort based on the hash to detect duplicate
            h1.cmp(&h2)
                // Then, based on the features, to keep those with less.
                // While some features imply others (avx2 imply avx),
                // the hashes should be different. There should not be a case
                // with same number of features lead to same binary hash.
                .then_with(|| b1.cpufeatures.len().cmp(&b2.cpufeatures.len()))
        });

        binaries_desc.dedup_by(|h1, h2| h1.0 == h2.0);

        self.progress.finish_and_clear();

        let bins = binaries_desc.into_iter().map(|bd| bd.1).collect();
        Ok(Artifacts {bins})
    }

    /// Compile a single package from the workspace
    /// for a single set of CPU features
    /// returns the hash of a binary for dedup purposes
    /// and a description of it.
    /// We choose SHA256 for its ubiquitous hardware acceleration on CPUs
    fn compile_pkg(
        &self,
        cargo_toml: &Path,
        rustflags: &str,
        pkg_features: &str,
        cpu_features: &CpuFeatures,
    ) -> anyhow::Result<([u8; 32], BinaryDesc)> {
        let arch_flags = cpu_features.to_compiler_flags();
        // TODO: pass the name of a CPU if any was specified for example x86-64-v3 (+avx,+avx2,+bmi,+bmi2,...)
        self.progress.println(format!(
            "{:>12} {}",
            style("Compiling").bold().green(),
            if arch_flags.len() > 0 { &arch_flags } else { "default fallback" }
        ));

        let target_string = self.target.to_string();

        let rust_flags = format!("{rustflags} -Ctarget-feature={arch_flags}");
        let cargo = CargoBuild::new()
            .arg(format!("--profile={}", self.profile))
            .target(&target_string)
            .manifest_path(cargo_toml)
            .args(&self.cargo_args)
            .env("RUSTFLAGS", rust_flags);

        let cargo = if self.pkg_features.all_features {
            cargo.all_features()
        } else if self.pkg_features.no_default_features {
            cargo.no_default_features()
        } else {
            cargo.features(&pkg_features)
        };

        let cargo = cargo.exec()?;
        let bin_path = cargo
            .find_executable()?
            .ok_or_else(|| anyhow::anyhow!("Failed to find a binary"))?;

        self.progress.inc(1);

        let filename = format!("bin-{}", cpu_features.iter().join("_"));

        let output_path_parent = self.target_dir.join(&target_string).join(&self.profile_dir);
        let mut output_path = output_path_parent.join(filename);
        output_path.set_extension(std::env::consts::EXE_EXTENSION);

        std::fs::create_dir_all(&output_path_parent).with_context(|| {
            format!(
                "Failed to create directory `{}`",
                output_path_parent.display()
            )
        })?;
        std::fs::copy(&bin_path, &output_path).with_context(|| {
            format!(
                "Failed to copy `{}` to `{}`",
                bin_path.display(),
                output_path.display()
            )
        })?;

        let hash = std::fs::read(&output_path).map(Sha256::digest)?;

        let desc = BinaryDesc {
            path: output_path,
            cpufeatures: cpu_features.iter().cloned().collect(),
            original_filename: bin_path.file_name().map(ToOwned::to_owned),
        };

        Ok((hash.into(), desc))
    }
}
