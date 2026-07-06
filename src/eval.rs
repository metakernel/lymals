use thiserror::Error;

use crate::config::EvaluationSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationState {
    Disabled,
    Reserved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationRequest<'a> {
    pub expression: &'a str,
    pub settings: &'a EvaluationSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationResult {
    pub state: EvaluationState,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EvaluationError {
    #[error("evaluation is disabled; lymals v1 is parse-only")]
    Disabled,
    #[error("evaluation is reserved for a future sandboxed extension and is not shipped in v1")]
    Reserved,
}

pub fn evaluate(request: EvaluationRequest<'_>) -> Result<EvaluationResult, EvaluationError> {
    let _ = request.expression;
    if !request.settings.enabled {
        return Err(EvaluationError::Disabled);
    }

    Err(EvaluationError::Reserved)
}

pub fn state(settings: &EvaluationSettings) -> EvaluationState {
    if settings.enabled {
        EvaluationState::Reserved
    } else {
        EvaluationState::Disabled
    }
}

#[cfg(test)]
mod tests {
    use crate::config::EvaluationSettings;

    use super::{EvaluationError, EvaluationRequest, EvaluationState, evaluate, state};

    #[test]
    fn default_evaluation_is_disabled_and_executes_nothing() {
        let settings = EvaluationSettings::default();

        assert_eq!(state(&settings), EvaluationState::Disabled);
        assert_eq!(
            evaluate(EvaluationRequest {
                expression: "os.execute('whoami')",
                settings: &settings,
            }),
            Err(EvaluationError::Disabled)
        );
    }

    #[test]
    fn enabled_evaluation_is_still_reserved_in_v1() {
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
}
