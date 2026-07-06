use lumals::{
    config::{EvaluationSettings, LumalsConfig},
    eval::{EvaluationError, EvaluationRequest, EvaluationState, evaluate, state},
};
use std::{fs, path::Path};

#[test]
fn default_config_keeps_evaluation_disabled() {
    let config = LumalsConfig::default();

    assert!(!config.evaluation.enabled);
    assert_eq!(state(&config.evaluation), EvaluationState::Disabled);
    assert_eq!(
        evaluate(EvaluationRequest {
            expression: "os.execute('whoami')",
            settings: &config.evaluation,
        }),
        Err(EvaluationError::Disabled)
    );
}

#[test]
fn opt_in_setting_is_reserved_not_executed_in_v1() {
    let settings = EvaluationSettings { enabled: true };

    assert_eq!(state(&settings), EvaluationState::Reserved);
    assert_eq!(
        evaluate(EvaluationRequest {
            expression: "1 + 1",
            settings: &settings,
        }),
        Err(EvaluationError::Reserved)
    );
}

#[test]
fn shipped_default_feature_handlers_do_not_call_evaluation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let feature_files = [
        "src/diagnostics.rs",
        "src/completion.rs",
        "src/hover.rs",
        "src/navigation.rs",
        "src/references.rs",
        "src/rename.rs",
        "src/symbols.rs",
        "src/formatting.rs",
        "src/semantic_tokens.rs",
        "src/folding.rs",
        "src/selection_ranges.rs",
        "src/code_actions.rs",
        "src/commands.rs",
        "src/imports.rs",
        "src/index.rs",
        "src/workspace.rs",
    ];
    let forbidden_markers = [
        "evaluate(",
        "EvaluationRequest",
        "crate::eval",
        "eval::evaluate",
        "eval::state",
    ];

    for relative_path in feature_files {
        let source = fs::read_to_string(root.join(relative_path))
            .unwrap_or_else(|error| panic!("failed to read {relative_path}: {error}"));

        for marker in forbidden_markers {
            assert!(
                !source.contains(marker),
                "{relative_path} must remain parse-only and not reference evaluation marker `{marker}`"
            );
        }
    }
}
