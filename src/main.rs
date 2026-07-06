mod cli;
mod logging;

use std::{
    ffi::OsString,
    io::{self, Write},
    process::ExitCode,
};

use clap::Parser;
use tokio::runtime::Builder;
use tower_lsp::Server;

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

    match command_mode(&cli) {
        CommandMode::Version => {
            let _ = writeln!(stdout, "{}", lymals::version_banner());
            return ExitCode::SUCCESS;
        }
        CommandMode::PrintConfigSchema => {
            let _ = writeln!(stdout, "{}", lymals::config::config_schema_json());
            return ExitCode::SUCCESS;
        }
        CommandMode::Stdio => {}
    }

    logging::install_panic_hook(cli.log_file.clone());

    if let Err(error) = logging::init_logging(cli.log_file.as_deref()) {
        let _ = writeln!(stderr, "failed to initialize logging: {error:#}");
        return ExitCode::FAILURE;
    }

    run_stdio(cli, stdout, stderr)
}

fn run_stdio(_cli: Cli, _stdout: &mut dyn Write, stderr: &mut dyn Write) -> ExitCode {
    let runtime = match Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = writeln!(stderr, "failed to start async runtime: {error:#}");
            return ExitCode::FAILURE;
        }
    };

    runtime.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = lymals::server::service();
        Server::new(stdin, stdout, socket).serve(service).await;
    });

    ExitCode::SUCCESS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandMode {
    Stdio,
    Version,
    PrintConfigSchema,
}

fn command_mode(cli: &Cli) -> CommandMode {
    if cli.version {
        CommandMode::Version
    } else if cli.print_config_schema {
        CommandMode::PrintConfigSchema
    } else {
        CommandMode::Stdio
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{CommandMode, command_mode, run};
    use crate::cli::Cli;

    #[test]
    fn version_flag_prints_banner() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(["lymals", "--version"], &mut stdout, &mut stderr);

        assert_eq!(code, std::process::ExitCode::SUCCESS);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            format!("{}\n", lymals::version_banner())
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn config_schema_flag_prints_schema() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(
            ["lymals", "--print-config-schema"],
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(code, std::process::ExitCode::SUCCESS);
        assert!(String::from_utf8(stdout).unwrap().contains("\"$schema\""));
        assert!(stderr.is_empty());
    }

    #[test]
    fn version_flag_exits_before_logging_initialization() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(
            [
                "lymals",
                "--version",
                "--log-file",
                "?:\\invalid\\lymals.log",
            ],
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(code, std::process::ExitCode::SUCCESS);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            format!("{}\n", lymals::version_banner())
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn config_schema_flag_exits_before_logging_initialization() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let code = run(
            [
                "lymals",
                "--print-config-schema",
                "--log-file",
                "?:\\invalid\\lymals.log",
            ],
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(code, std::process::ExitCode::SUCCESS);
        assert!(String::from_utf8(stdout).unwrap().contains("\"$schema\""));
        assert!(stderr.is_empty());
    }

    #[test]
    fn default_stdio_mode_does_not_write_to_stdout() {
        let cli = Cli::parse_from(["lymals"]);
        assert_eq!(command_mode(&cli), CommandMode::Stdio);
    }

    #[test]
    fn explicit_stdio_flag_is_a_quiet_synonym() {
        let cli = Cli::parse_from(["lymals", "--stdio"]);
        assert_eq!(command_mode(&cli), CommandMode::Stdio);
    }

    #[test]
    fn version_takes_priority_over_stdio_mode() {
        let cli = Cli::parse_from(["lymals", "--stdio", "--version"]);
        assert_eq!(command_mode(&cli), CommandMode::Version);
    }

    #[test]
    fn config_schema_takes_priority_over_stdio_mode() {
        let cli = Cli::parse_from(["lymals", "--stdio", "--print-config-schema"]);
        assert_eq!(command_mode(&cli), CommandMode::PrintConfigSchema);
    }
}
