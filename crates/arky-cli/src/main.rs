//! The arky command line.

mod cli;
mod run;

use std::io;
use std::process::ExitCode;

use clap::Parser as _;
use os_shim::real::RealSystem;
use tokio::runtime::Builder;

use crate::cli::Cli;
use crate::run::run;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let system = RealSystem::new();
    let runtime = match Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("arky: {error}");
            return ExitCode::from(4);
        }
    };
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let mut stdin = io::stdin();
    runtime.block_on(run(&cli, &system, &mut stdin, &mut stdout, &mut stderr))
}
