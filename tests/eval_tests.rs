use lumals::{
    config::{EvaluationSettings, LumalsConfig},
    eval::{EvaluationError, EvaluationRequest, EvaluationState, evaluate, state},
};

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
