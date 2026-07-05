use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CONFIG_SECTION: &str = "lumals";
pub const CONFIG_SCHEMA_ID: &str = "https://lumals.dev/schemas/lumals.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct LumalsConfig {
    #[serde(default)]
    pub diagnostics: DiagnosticsSettings,
    #[serde(default)]
    pub formatting: FormattingSettings,
    #[serde(default)]
    pub imports: ImportsSettings,
    #[serde(default)]
    pub semantic_tokens: SemanticTokensSettings,
    #[serde(default)]
    pub completion: CompletionSettings,
    #[serde(default)]
    pub inlay_hints: InlayHintsSettings,
    #[serde(default)]
    pub evaluation: EvaluationSettings,
    #[serde(default)]
    pub log_level: LogLevel,
    #[serde(default)]
    pub parser_backend: ParserBackend,
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    pub index_workspace: bool,
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    pub follow_imports_in_index: bool,
    #[serde(default)]
    #[schemars(default)]
    pub allowed_roots: Vec<String>,
    #[serde(default = "default_allowed_schemes")]
    #[schemars(default = "default_allowed_schemes")]
    pub allowed_schemes: Vec<String>,
    #[serde(default)]
    #[schemars(default)]
    pub allow_absolute_file_uris: bool,
    #[serde(default)]
    #[schemars(default)]
    pub exclude_globs: Vec<String>,
    #[serde(default = "default_max_resolve_depth")]
    #[schemars(default = "default_max_resolve_depth")]
    pub max_resolve_depth: u32,
    #[serde(default = "default_max_resolved_edges_per_file")]
    #[schemars(default = "default_max_resolved_edges_per_file")]
    pub max_resolved_edges_per_file: u32,
    #[serde(default = "default_max_indexed_files_per_workspace")]
    #[schemars(default = "default_max_indexed_files_per_workspace")]
    pub max_indexed_files_per_workspace: u32,
    #[serde(default = "default_max_indexed_file_bytes")]
    #[schemars(default = "default_max_indexed_file_bytes")]
    pub max_indexed_file_bytes: u32,
}

impl Default for LumalsConfig {
    fn default() -> Self {
        Self {
            diagnostics: DiagnosticsSettings::default(),
            formatting: FormattingSettings::default(),
            imports: ImportsSettings::default(),
            semantic_tokens: SemanticTokensSettings::default(),
            completion: CompletionSettings::default(),
            inlay_hints: InlayHintsSettings::default(),
            evaluation: EvaluationSettings::default(),
            log_level: LogLevel::default(),
            parser_backend: ParserBackend::default(),
            index_workspace: default_true(),
            follow_imports_in_index: default_true(),
            allowed_roots: Vec::new(),
            allowed_schemes: default_allowed_schemes(),
            allow_absolute_file_uris: false,
            exclude_globs: Vec::new(),
            max_resolve_depth: default_max_resolve_depth(),
            max_resolved_edges_per_file: default_max_resolved_edges_per_file(),
            max_indexed_files_per_workspace: default_max_indexed_files_per_workspace(),
            max_indexed_file_bytes: default_max_indexed_file_bytes(),
        }
    }
}

impl LumalsConfig {
    pub fn from_lsp_value(value: &Value) -> serde_json::Result<Self> {
        match value {
            Value::Null => Ok(Self::default()),
            Value::Object(object) => {
                if let Some(section_value) = object.get(CONFIG_SECTION) {
                    serde_json::from_value(section_value.clone())
                } else {
                    serde_json::from_value(Value::Object(object.clone()))
                }
            }
            _ => serde_json::from_value(value.clone()),
        }
    }
}

pub fn config_schema() -> Value {
    let mut value =
        serde_json::to_value(schema_for!(LumalsConfig)).expect("schema should serialize");

    let object = value
        .as_object_mut()
        .expect("config schema root should be an object");
    object.insert(
        "$id".to_string(),
        Value::String(CONFIG_SCHEMA_ID.to_string()),
    );
    object.insert(
        "title".to_string(),
        Value::String("lumals config".to_string()),
    );
    value
}

pub fn config_schema_json() -> String {
    serde_json::to_string_pretty(&config_schema()).expect("schema should be valid json")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DiagnosticsSettings {
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    pub enabled: bool,
}

impl Default for DiagnosticsSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FormattingSettings {
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    pub enabled: bool,
}

impl Default for FormattingSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ImportsSettings {
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    pub enabled: bool,
}

impl Default for ImportsSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct SemanticTokensSettings {
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    pub enabled: bool,
}

impl Default for SemanticTokensSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CompletionSettings {
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    pub enabled: bool,
}

impl Default for CompletionSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct InlayHintsSettings {
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    #[schemars(default)]
    pub inferred_types: bool,
    #[serde(default)]
    #[schemars(default)]
    pub key_paths: bool,
    #[serde(default)]
    #[schemars(default)]
    pub let_bindings: bool,
    #[serde(default)]
    #[schemars(default)]
    pub profile_effects: bool,
    #[serde(default)]
    #[schemars(default)]
    pub import_resolution: bool,
}

impl Default for InlayHintsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            inferred_types: false,
            key_paths: false,
            let_bindings: false,
            profile_effects: false,
            import_resolution: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EvaluationSettings {
    #[serde(default)]
    #[schemars(default)]
    pub enabled: bool,
}

impl Default for EvaluationSettings {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub enum ParserBackend {
    #[default]
    Auto,
    Fallback,
    Upstream,
}

fn default_true() -> bool {
    true
}

fn default_allowed_schemes() -> Vec<String> {
    vec!["file".to_string()]
}

fn default_max_resolve_depth() -> u32 {
    16
}

fn default_max_resolved_edges_per_file() -> u32 {
    256
}

fn default_max_indexed_files_per_workspace() -> u32 {
    10_000
}

fn default_max_indexed_file_bytes() -> u32 {
    1_048_576
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CONFIG_SCHEMA_ID, CONFIG_SECTION, LumalsConfig, config_schema};

    #[test]
    fn defaults_match_parse_only_policy() {
        let config = LumalsConfig::default();

        assert!(config.diagnostics.enabled);
        assert!(config.formatting.enabled);
        assert!(config.imports.enabled);
        assert!(config.semantic_tokens.enabled);
        assert!(config.completion.enabled);
        assert!(config.inlay_hints.enabled);
        assert!(!config.evaluation.enabled);
        assert!(config.index_workspace);
        assert!(config.follow_imports_in_index);
        assert_eq!(config.allowed_schemes, ["file"]);
        assert_eq!(config.max_resolve_depth, 16);
        assert_eq!(config.max_resolved_edges_per_file, 256);
        assert_eq!(config.max_indexed_files_per_workspace, 10_000);
        assert_eq!(config.max_indexed_file_bytes, 1_048_576);
    }

    #[test]
    fn parses_direct_and_wrapped_configuration_values() {
        let direct = LumalsConfig::from_lsp_value(&json!({
            "evaluation": { "enabled": true },
            "allowedSchemes": ["file", "untitled"]
        }))
        .unwrap();
        assert!(direct.evaluation.enabled);
        assert_eq!(direct.allowed_schemes, ["file", "untitled"]);

        let wrapped = LumalsConfig::from_lsp_value(&json!({
            CONFIG_SECTION: {
                "logLevel": "debug",
                "indexWorkspace": false
            }
        }))
        .unwrap();
        assert_eq!(wrapped.log_level, super::LogLevel::Debug);
        assert!(!wrapped.index_workspace);
    }

    #[test]
    fn generated_schema_includes_expected_defaults() {
        let schema = config_schema();
        let root = schema.as_object().unwrap();
        let properties = root["properties"].as_object().unwrap();

        assert_eq!(root["$id"], CONFIG_SCHEMA_ID);
        assert_eq!(properties["allowedSchemes"]["default"][0], "file");
        assert_eq!(properties["evaluation"]["default"]["enabled"], false);
        assert_eq!(properties["maxResolveDepth"]["default"], 16);
    }
}
