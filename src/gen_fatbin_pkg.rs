use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use escargot::CargoBuild;
use indoc::formatdoc;

use crate::cargo_msg_parser::CommandMessagesExt;

pub struct FatbinCrate {
    outdir: PathBuf,
    cargo_toml: PathBuf,
}

impl FatbinCrate {
    pub(crate) fn generate(outdir: PathBuf) -> anyhow::Result<Self> {
        let name = &format!("multiarch-dispatch-autogen");
        let root_dir = outdir.join(name);
        let srcdir = root_dir.join("src");
        let cargo_toml = root_dir.join("Cargo.toml");
        let main_rs = srcdir.join("main.rs");
        let local_dispatcher = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("multiarch-dispatch");

        let dispatcher = format!(
            r#"multiarch-dispatch = {{ path = "{}" }}"#,
            local_dispatcher.to_string_lossy().replace('\\', "/")
        );

        let manifest = formatdoc!(
            r#"
            [package]
            name = "{name}"
            publish = false
            edition = "2021"

            [dependencies]
            {dispatcher}

            [profile.release]
            lto = true
            strip = "symbols"
            opt-level = "z"
            codegen-units = 1
            panic = "abort"

            [workspace]
        "#
        );

        let main = formatdoc!(
            r#"
            #![no_main]
            pub use multiarch_dispatch::main;
        "#
        );

        std::fs::create_dir_all(&srcdir)?;
        std::fs::write(&cargo_toml, manifest)?;
        std::fs::write(main_rs, main)?;

        Ok(Self { outdir, cargo_toml })
    }

    pub(crate) fn cargo_build(
        &self,
        target: &str,
        artifacts_json_path: &Path,
        original_filename: &OsStr,
    ) -> anyhow::Result<PathBuf> {
        // We do not propagate `CARGO_UNSTABLE_BUILD_STD` since if `panic_abort` is not
        // specified, the build of the runner will fail (since its profile specifies `panic=abort`).
        // A proper fix could be to clear the whole environment before spawning this `cargo build`,
        // but until `CargoBuild` exposes the `Command` or this function, we can only do this.
        let cargo = CargoBuild::new()
            .release()
            .target(target)
            .target_dir(&self.outdir)
            .manifest_path(&self.cargo_toml)
            .env_remove("CARGO_UNSTABLE_BUILD_STD")
            .env("MULTIARCH_ARTIFACTS", artifacts_json_path);

        let cargo = cargo
            .exec()
            .context("Failed to execute cargo to build the fatbin")?;

        let bin_path = cargo
            .find_executable()?
            .ok_or_else(|| anyhow::anyhow!("Failed to build the runner"))?;

        let mut output_path = bin_path.clone();
        output_path.set_file_name(original_filename);
        fs::rename(&bin_path, &output_path)?;

        Ok(output_path)
    }
}
