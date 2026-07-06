use std::path::PathBuf;

use clap::{ArgAction, Parser};

#[derive(Debug, Clone, Parser, PartialEq, Eq)]
#[command(
    name = "lymals",
    disable_version_flag = true,
    about = "Parse-only Lyma language server"
)]
pub struct Cli {
    /// Explicit stdio transport synonym for editor/client configuration.
    #[arg(long, action = ArgAction::SetTrue)]
    pub stdio: bool,

    /// Print the binary version and exit.
    #[arg(short = 'V', long = "version", action = ArgAction::SetTrue)]
    pub version: bool,

    /// Print the JSON config schema and exit.
    #[arg(long = "print-config-schema", action = ArgAction::SetTrue)]
    pub print_config_schema: bool,

    /// Write structured logs to a file instead of stderr.
    #[arg(long = "log-file", value_name = "PATH")]
    pub log_file: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use serde_json::Value;

    use super::Cli;

    #[test]
    fn parses_cli_flags() {
        let cli = Cli::parse_from([
            "lymals",
            "--stdio",
            "--version",
            "--print-config-schema",
            "--log-file",
            "lymals.log",
        ]);

        assert!(cli.stdio);
        assert!(cli.version);
        assert!(cli.print_config_schema);
        assert_eq!(
            cli.log_file.as_deref(),
            Some(std::path::Path::new("lymals.log"))
        );
    }

    #[test]
    fn config_schema_is_valid_json() {
        let schema: Value = serde_json::from_str(&lymals::config::config_schema_json()).unwrap();

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["allowedSchemes"]["default"][0], "file");
        assert_eq!(
            schema["properties"]["evaluation"]["default"]["enabled"],
            false
        );
        assert_eq!(schema["properties"]["maxResolveDepth"]["default"], 16);
    }
}
