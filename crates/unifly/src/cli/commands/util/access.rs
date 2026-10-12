use unifly_api::Controller;

use crate::cli::error::CliError;

pub async fn ensure_integration_access(
    controller: &Controller,
    operation: &str,
) -> Result<(), CliError> {
    // The demo fleet holds every store-backed collection; the --demo-fleet
    // allowlist keeps commands that need a live API from getting here.
    if controller.is_demo() || controller.has_integration_access().await {
        return Ok(());
    }

    Err(CliError::Unsupported {
        operation: operation.into(),
        required: "Integration API".into(),
    })
}

pub async fn ensure_session_access(
    controller: &Controller,
    operation: &str,
) -> Result<(), CliError> {
    if controller.is_demo() || controller.has_session_access().await {
        return Ok(());
    }

    Err(CliError::Unsupported {
        operation: operation.into(),
        required: "session or hybrid authentication".into(),
    })
}
