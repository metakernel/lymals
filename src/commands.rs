use serde_json::Value;
use tower_lsp::{jsonrpc::Error, lsp_types::Url};

pub const RESTART_INDEX: &str = "lumals.restartIndex";
pub const SHOW_SYNTAX_TREE: &str = "lumals.showSyntaxTree";
pub const SHOW_CONFIG: &str = "lumals.showConfig";
pub const FORMAT_WORKSPACE_FILE: &str = "lumals.formatWorkspaceFile";
pub const EXPLAIN_DIAGNOSTIC: &str = "lumals.explainDiagnostic";

pub const ALL: &[&str] = &[
    RESTART_INDEX,
    SHOW_SYNTAX_TREE,
    SHOW_CONFIG,
    FORMAT_WORKSPACE_FILE,
    EXPLAIN_DIAGNOSTIC,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    RestartIndex,
    ShowSyntaxTree,
    ShowConfig,
    FormatWorkspaceFile,
    ExplainDiagnostic,
}

impl Command {
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            RESTART_INDEX => Some(Self::RestartIndex),
            SHOW_SYNTAX_TREE => Some(Self::ShowSyntaxTree),
            SHOW_CONFIG => Some(Self::ShowConfig),
            FORMAT_WORKSPACE_FILE => Some(Self::FormatWorkspaceFile),
            EXPLAIN_DIAGNOSTIC => Some(Self::ExplainDiagnostic),
            _ => None,
        }
    }

    #[must_use]
    pub fn registration() -> Vec<String> {
        ALL.iter().map(|command| (*command).to_string()).collect()
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::RestartIndex => RESTART_INDEX,
            Self::ShowSyntaxTree => SHOW_SYNTAX_TREE,
            Self::ShowConfig => SHOW_CONFIG,
            Self::FormatWorkspaceFile => FORMAT_WORKSPACE_FILE,
            Self::ExplainDiagnostic => EXPLAIN_DIAGNOSTIC,
        }
    }
}

pub fn expect_no_arguments(command: Command, arguments: &[Value]) -> Result<(), Error> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(Error::invalid_params(format!(
            "{} does not accept arguments",
            command.name()
        )))
    }
}

pub fn parse_uri_argument(command: Command, arguments: &[Value]) -> Result<Url, Error> {
    let Some(argument) = arguments.first() else {
        return Err(Error::invalid_params(format!(
            "{} expects a single uri argument",
            command.name()
        )));
    };

    if arguments.len() != 1 {
        return Err(Error::invalid_params(format!(
            "{} expects a single uri argument",
            command.name()
        )));
    }

    let uri = match argument {
        Value::String(uri) => uri.as_str(),
        Value::Object(object) => object
            .get("uri")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::invalid_params(format!(
                    "{} expects a single uri argument",
                    command.name()
                ))
            })?,
        _ => {
            return Err(Error::invalid_params(format!(
                "{} expects a single uri argument",
                command.name()
            )));
        }
    };

    Url::parse(uri).map_err(|_| Error::invalid_params("uri must be a valid absolute URI"))
}

pub fn parse_diagnostic_code_argument(arguments: &[Value]) -> Result<String, Error> {
    let Some(argument) = arguments.first() else {
        return Err(Error::invalid_params(
            "lumals.explainDiagnostic expects a single diagnostic code argument",
        ));
    };

    if arguments.len() != 1 {
        return Err(Error::invalid_params(
            "lumals.explainDiagnostic expects a single diagnostic code argument",
        ));
    }

    let code = match argument {
        Value::String(code) => code.as_str(),
        Value::Object(object) => object
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::invalid_params(
                    "lumals.explainDiagnostic expects a single diagnostic code argument",
                )
            })?,
        _ => {
            return Err(Error::invalid_params(
                "lumals.explainDiagnostic expects a single diagnostic code argument",
            ));
        }
    };

    let code = code.trim();
    if code.is_empty() {
        return Err(Error::invalid_params("diagnostic code must not be empty"));
    }

    Ok(code.to_string())
}

pub fn diagnostic_explanation(code: &str) -> Option<(&'static str, &'static str)> {
    Some(match code {
        "L001" => ("NUL byte", "Remove embedded NUL bytes from the source file."),
        "L002" => (
            "Duplicate mapping key",
            "Rename or remove the later key so each mapping key is unique.",
        ),
        "L003" => (
            "Tab indentation",
            "Replace indentation tabs with spaces; lumals expects multiples of two spaces.",
        ),
        "L004" => (
            "Odd indentation width",
            "Use indentation levels in multiples of two spaces.",
        ),
        "L005" => (
            "Invalid indentation increase",
            "Only indent one level deeper when the previous line starts a child block.",
        ),
        "L006" => (
            "Unmatched indentation",
            "Dedent to a previously established indentation level.",
        ),
        "L007" => ("Malformed directive", "Use @name with an ASCII alphanumeric directive name."),
        "L008" => (
            "Missing import target",
            "Provide a relative path or file: URI after the directive.",
        ),
        "L009" => (
            "Unknown directive",
            "Use a supported directive such as @luma, @schema, @import, or @include.",
        ),
        "L010" => (
            "Disallowed import scheme",
            "Use a relative path or an allowed file: URI only.",
        ),
        "L011" => (
            "Parent traversal in file URI",
            "Remove .. segments so the resolved file stays within allowed roots.",
        ),
        "L012" => (
            "Absolute path blocked",
            "Use a workspace-relative import path unless absolute file URIs are enabled.",
        ),
        "L013" => (
            "Relative path escapes root",
            "Keep relative import paths inside the configured workspace roots.",
        ),
        "L014" => (
            "let alias blocked",
            "Parse-only mode does not support let aliases; rewrite without 'as'.",
        ),
        "L015" => (
            "Unterminated string",
            "Close the quoted string before the end of the line.",
        ),
        "L016" => (
            "Unterminated lua block",
            "Close lua{ ... } blocks on the same logical block.",
        ),
        "L017" => (
            "Unterminated fenced lua block",
            "Add a closing ``` fence for the lua block.",
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        Command, EXPLAIN_DIAGNOSTIC, FORMAT_WORKSPACE_FILE, SHOW_CONFIG, SHOW_SYNTAX_TREE,
        diagnostic_explanation, expect_no_arguments, parse_diagnostic_code_argument,
        parse_uri_argument,
    };

    #[test]
    fn registers_expected_commands() {
        let commands = Command::registration();

        assert!(commands.contains(&SHOW_SYNTAX_TREE.to_string()));
        assert!(commands.contains(&SHOW_CONFIG.to_string()));
        assert!(commands.contains(&FORMAT_WORKSPACE_FILE.to_string()));
        assert!(commands.contains(&EXPLAIN_DIAGNOSTIC.to_string()));
    }

    #[test]
    fn uri_argument_accepts_string_and_object_forms() {
        let string = parse_uri_argument(
            Command::ShowSyntaxTree,
            &[json!("file:///workspace/test.luma")],
        )
        .unwrap();
        let object = parse_uri_argument(
            Command::ShowSyntaxTree,
            &[json!({ "uri": "file:///workspace/test.luma" })],
        )
        .unwrap();

        assert_eq!(string, object);
    }

    #[test]
    fn uri_argument_rejects_missing_or_invalid_values() {
        assert!(parse_uri_argument(Command::FormatWorkspaceFile, &[]).is_err());
        assert!(parse_uri_argument(Command::FormatWorkspaceFile, &[json!({})]).is_err());
        assert!(parse_uri_argument(Command::FormatWorkspaceFile, &[json!("not-a-uri")]).is_err());
    }

    #[test]
    fn diagnostic_code_argument_validates_shape() {
        assert_eq!(parse_diagnostic_code_argument(&[json!("L003")]).unwrap(), "L003");
        assert!(parse_diagnostic_code_argument(&[]).is_err());
        assert!(parse_diagnostic_code_argument(&[json!({})]).is_err());
    }

    #[test]
    fn diagnostic_explanations_are_known_for_validation_codes() {
        assert!(diagnostic_explanation("L003").is_some());
        assert!(diagnostic_explanation("L999").is_none());
    }

    #[test]
    fn no_argument_commands_reject_extra_arguments() {
        assert!(expect_no_arguments(Command::ShowConfig, &[json!(true)]).is_err());
        assert!(expect_no_arguments(Command::RestartIndex, &[]).is_ok());
        assert_eq!(Command::parse(EXPLAIN_DIAGNOSTIC), Some(Command::ExplainDiagnostic));
    }
}
