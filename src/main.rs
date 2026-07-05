mod cli;
mod logging;

use std::{
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
};

use clap::Parser;

use crate::cli::Cli;

fn main() -> ExitCode {
    run(std::env::args_os(), &mut io::stdout(), &mut io::stderr())
}

fn run<I, T>(args: I, stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            if error.use_stderr() {
                let _ = writeln!(stderr, "{error}");
            } else {
                let _ = writeln!(stdout, "{error}");
            }
            return match error.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                    ExitCode::SUCCESS
                }
                _ => ExitCode::FAILURE,
            };
        }
    };

    if cli.version {
        let _ = writeln!(stdout, "{}", lumals::version_banner());
        return ExitCode::SUCCESS;
    }

    if cli.print_config_schema {
        let _ = writeln!(stdout, "{}", lumals::config::config_schema_json());
        return ExitCode::SUCCESS;
    }

    logging::install_panic_hook(cli.log_file.clone());

    if let Err(error) = logging::init_logging(cli.log_file.as_deref()) {
        let _ = writeln!(stderr, "failed to initialize logging: {error:#}");
        return ExitCode::FAILURE;
    }

    run_stdio(cli, stdout, stderr)
}

fn run_stdio(_cli: Cli, _stdout: &mut dyn Write, _stderr: &mut dyn Write) -> ExitCode {
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn version_flag_prints_banner() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(["lumals", "--version"], &mut stdout, &mut stderr);

        assert_eq!(code, std::process::ExitCode::SUCCESS);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            format!("{}\n", lumals::version_banner())
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn config_schema_flag_prints_schema() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(
            ["lumals", "--print-config-schema"],
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(code, std::process::ExitCode::SUCCESS);
        assert!(String::from_utf8(stdout).unwrap().contains("\"$schema\""));
        assert!(stderr.is_empty());
    }

    #[test]
    fn default_stdio_mode_does_not_write_to_stdout() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(["lumals"], &mut stdout, &mut stderr);

        assert_eq!(code, std::process::ExitCode::SUCCESS);
        assert!(stdout.is_empty());
    }

    #[test]
    fn explicit_stdio_flag_is_a_quiet_synonym() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(["lumals", "--stdio"], &mut stdout, &mut stderr);

        assert_eq!(code, std::process::ExitCode::SUCCESS);
        assert!(stdout.is_empty());
    }
}
