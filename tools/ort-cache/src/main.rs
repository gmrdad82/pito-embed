use std::io::IsTerminal;
use std::process::ExitCode;

use ort_cache::args::{self, Parsed};
use ort_cache::{Inputs, Outcome, land};

fn main() -> ExitCode {
    let opts = match args::parse(
        std::env::args_os()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned()),
    ) {
        Parsed::Run(opts) => opts,
        Parsed::Help => {
            print!("{}", args::HELP);
            return ExitCode::SUCCESS;
        }
        Parsed::Usage(err) => {
            let color = std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
                && std::io::stderr().is_terminal();
            eprint!("{}", err.render(color));
            return ExitCode::from(2);
        }
    };
    match ort_cache::run(&opts, &Inputs::from_env(), &land::Net) {
        Ok(Outcome::Ready(dest)) => {
            println!("{}", dest.display());
            ExitCode::SUCCESS
        }
        Ok(Outcome::Missing(dest)) => {
            println!("{}", dest.display());
            ExitCode::FAILURE
        }
        Err(why) => {
            eprintln!("ort-cache: {why}");
            ExitCode::FAILURE
        }
    }
}
