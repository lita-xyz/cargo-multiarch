use anyhow::{self, Ok};
use clap::Parser;
use compile_multiarch::Multiarch;

use crate::rustc_queries::Rustc;

mod cargo_config_loader;
mod cargo_msg_parser;
mod cli;
mod compile_multiarch;
mod gen_fatbin_pkg;
mod rustc_queries;

fn main() -> anyhow::Result<()> {
    let cli::Cargo::Multiarch(args) = cli::Cargo::parse();

    if let Some(query) = args.print {
        let info = match query {
            cli::Print::TargetList => Rustc::get_target_list(),
            cli::Print::TargetCpus => Rustc::get_cpus_for_target(args.target.as_deref()),
            cli::Print::TargetCpuFeatures => Rustc::get_cpufeatures_for_humans(
                args.target.as_deref(),
                args.target_cpu.as_deref(),
            ),
        }?;
        println!("{}", info);
        return Ok(());
    }

    anyhow::ensure!(
        Rustc::is_nightly(),
        "You must run cargo multivers with Rust nightly channel. For example, you can run: `cargo +nightly multivers`"
    );

    Multiarch::from_args(args)?.compile_workspace()
}
