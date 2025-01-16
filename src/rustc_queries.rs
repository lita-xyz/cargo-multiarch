use std::io::BufRead;
use std::path::PathBuf;
use std::process::Command;
use std::sync::LazyLock;

use anyhow;
use indoc::formatdoc;

static RUSTC: LazyLock<PathBuf> = LazyLock::new(|| {
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .and_then(|path| {
            path.parent().map(|bin| {
                bin.join("rustc")
                    .with_extension(std::env::consts::EXE_EXTENSION)
            })
        })
        .unwrap_or_else(|| "rustc".into())
});

/// Wrapper around the `rustc` command
pub struct Rustc;

impl Rustc {
    fn command() -> Command {
        Command::new(RUSTC.as_path())
    }

    /// Returns true if rustc is on the nightly release channel
    pub fn is_nightly() -> bool {
        let Ok(output) = Self::command().arg("-vV").output() else {
            return false;
        };

        let release = output
            .stdout
            .lines()
            .map_while(Result::ok)
            .find_map(|line| line.strip_prefix("release: ").map(ToOwned::to_owned))
            .unwrap_or_default();

        release.contains("nightly")
    }

    pub fn get_target_list() -> anyhow::Result<String> {
        let output = Self::command().args(["--print", "target-list"]).output()?;
        String::from_utf8(output.stdout).map_err(anyhow::Error::msg)
    }

    pub fn get_host_target() -> anyhow::Result<String> {
        let output = Self::command().arg("-vV").output()?;

        output
            .stdout
            .lines()
            .map_while(Result::ok)
            .find_map(|line| line.strip_prefix("host: ").map(ToOwned::to_owned))
            .ok_or_else(|| anyhow::anyhow!("cargo-multiarch: Failed to detect default target"))
    }

    pub(crate) fn target_triple_or_host(target_triple: Option<&str>) -> anyhow::Result<String> {
        // Hey dawg, I heard you liked to target triples
        if let Some(target_triple) = target_triple {
            Ok(target_triple.to_owned())
        } else {
            Self::get_host_target()
        }
    }

    pub fn get_cpus_for_target(target_triple: Option<&str>) -> anyhow::Result<String> {
        let target_triple = Self::target_triple_or_host(target_triple)?;
        let output = Self::command()
            .args(["--print=target-cpus", "--target", &target_triple])
            .output()?;
        Ok(formatdoc!(
            r#"
            {desc}
            [rustc-stdout]
            {stdout}
            [rustc-stderr]
            {stderr}
            [rustc-end]"#,
            desc = format!("Querying CPUs for target '{}'", target_triple),
            stdout = String::from_utf8(output.stdout).map_err(anyhow::Error::msg)?,
            stderr = String::from_utf8(output.stderr).map_err(anyhow::Error::msg)?,
        ))
    }

    fn get_host_cpu() -> anyhow::Result<String> {
        // Alternatively via build.rs: println!("cargo:rustc-env=CMA_TARGET_TRIPLE={}", std::env::var("TARGET").unwrap());
        // or target_lexicon::Host
        let output = Self::command().arg("--print=target-cpus").output()?;

        output
            .stdout
            .lines()
            .map_while(Result::ok)
            .find_map(|line| {
                // We don't need a full blown regex compiler for such a simple line
                line.strip_prefix(
                    "    native                  - Select the CPU of the current host (currently ",
                )?
                .strip_suffix(").")
                .map(ToOwned::to_owned)
            })
            .ok_or_else(|| anyhow::anyhow!("cargo-multiarch: Failed to detect host CPU"))
    }

    fn target_cpu_or_host(target_cpu: Option<&str>) -> anyhow::Result<String> {
        // Hey dawg, I heard you liked to target CPUs
        if let Some(target_cpu) = target_cpu {
            Ok(target_cpu.to_owned())
        } else {
            Self::get_host_cpu()
        }
    }

    pub fn get_cpufeatures_for_humans(
        target_triple: Option<&str>,
        target_cpu: Option<&str>,
    ) -> anyhow::Result<String> {
        let target_triple = Self::target_triple_or_host(target_triple)?;
        let target_cpu = Self::target_cpu_or_host(target_cpu)?;

        let output = Self::command()
            .arg("--print=target-features")
            .args(["--target", &target_triple])
            .arg(format!("-Ctarget-cpu={}", target_cpu))
            .output()?;

        let features = output
            .stdout
            .lines()
            .map_while(Result::ok)
            .take_while(|line| !line.starts_with("Code-generation features supported by LLVM"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(formatdoc!(
            r#"
            {desc}
            [rustc-stdout]
            {features}
            [rustc-stderr]
            {stderr}
            [rustc-end]"#,
            desc = format!("Querying features for CPU '{}'", target_cpu),
            features = features,
            stderr = String::from_utf8(output.stderr).map_err(anyhow::Error::msg)?,
        ))
    }
    pub fn get_cpufeatures_for_programs(
        target_triple: Option<&str>,
        target_cpu: Option<&str>,
    ) -> anyhow::Result<Vec<String>> {
        // Hey dawg, I heard you liked to target CPUs
        let target_triple = Self::target_triple_or_host(target_triple)?;
        let target_cpu = Self::target_cpu_or_host(target_cpu)?;

        let output = Self::command()
            .arg("--print=cfg")
            .args(["--target", &target_triple])
            .arg(format!("-Ctarget-cpu={}", target_cpu))
            .output()?;

        anyhow::ensure!(
            output.status.success() && output.stderr.is_empty(),
            "Invalid CPU '{target_cpu}'"
        );

        let features = output
            .stdout
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| {
                // We don't need a full blown regex compiler for such a simple line
                line.strip_prefix("target_feature=\"")?
                    .strip_suffix('"')
                    .map(ToOwned::to_owned)
            })
            .collect();
        Ok(features)
    }
}
