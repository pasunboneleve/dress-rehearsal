use crate::backends::BackendRequest;
use crate::context::RunContext;
use crate::scenarios::{
    Scenario, ScenarioDeployment, ScenarioDiscovery, ScenarioError, ScenarioPreparation,
    ScenarioTarget, ScenarioVerification,
};
use crate::steps::StepRunner;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendRehearsalScenarioConfig {
    deployment_root: PathBuf,
    working_directory: Option<PathBuf>,
}

impl BackendRehearsalScenarioConfig {
    pub fn new(deployment_root: impl Into<PathBuf>) -> Self {
        Self {
            deployment_root: deployment_root.into(),
            working_directory: None,
        }
    }

    pub fn deployment_root(&self) -> &Path {
        &self.deployment_root
    }

    pub fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    pub fn with_working_directory(mut self, working_directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(working_directory.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendRehearsalScenario {
    config: BackendRehearsalScenarioConfig,
}

impl BackendRehearsalScenario {
    pub fn new(config: BackendRehearsalScenarioConfig) -> Self {
        Self { config }
    }

    fn backend_request(&self) -> BackendRequest {
        let mut request = BackendRequest::new(self.config.deployment_root());
        if let Some(working_directory) = self.config.working_directory() {
            request = request.with_working_directory(working_directory);
        }
        request
    }
}

impl Scenario for BackendRehearsalScenario {
    fn name(&self) -> &'static str {
        "backend-rehearsal"
    }

    fn prepare(
        &self,
        _run_context: &RunContext,
        _runner: &StepRunner,
    ) -> Result<ScenarioPreparation, ScenarioError> {
        if !self.config.deployment_root().exists() {
            return Err(ScenarioError::invalid_configuration(
                self.name(),
                format!(
                    "deployment root does not exist: {}",
                    self.config.deployment_root().display()
                ),
            ));
        }

        let mut preparation = ScenarioPreparation::new(self.backend_request())
            .with_metadata("backend_input", "deployment_root");
        if let Some(working_directory) = self.config.working_directory() {
            preparation = preparation
                .with_metadata("working_directory", working_directory.display().to_string());
        }
        Ok(preparation)
    }

    fn discover(
        &self,
        _deployment: &ScenarioDeployment,
        _runner: &StepRunner,
    ) -> Result<ScenarioDiscovery, ScenarioError> {
        Ok(ScenarioDiscovery::new(ScenarioVerification::new(
            "apply completed",
            ScenarioTarget::NamedOutput {
                key: "lifecycle".to_string(),
                value: "applied".to_string(),
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{BackendRehearsalScenario, BackendRehearsalScenarioConfig};
    use crate::backends::{BackendOutputs, BackendRequest, BackendSession};
    use crate::context::{RunContext, RunId};
    use crate::scenarios::{Scenario, ScenarioDeployment, ScenarioTarget};
    use crate::steps::StepRunner;
    use crate::test_support::TestDir;
    use std::fs;
    use std::io;
    use std::path::Path;

    #[test]
    fn prepare_maps_generic_backend_inputs_into_scenario_boundary() -> io::Result<()> {
        let temp_dir = TestDir::new("backend-rehearsal-tests", "prepare")?;
        let deployment_root = temp_dir.path().join("deployment");
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-backend-0001"));
        fs::create_dir_all(&deployment_root)?;
        run_context.materialize()?;

        let scenario = BackendRehearsalScenario::new(
            BackendRehearsalScenarioConfig::new(&deployment_root)
                .with_working_directory(deployment_root.join("env/dev")),
        );

        let preparation = scenario
            .prepare(&run_context, &StepRunner::new())
            .map_err(io::Error::other)?;

        assert_eq!(
            preparation.backend_request().deployment_root(),
            deployment_root.as_path()
        );
        assert_eq!(preparation.preparation_steps().len(), 0);
        assert_eq!(preparation.cleanup_actions().len(), 0);
        assert_eq!(
            preparation.metadata().get("backend_input"),
            Some(&"deployment_root".to_string())
        );
        assert_eq!(
            preparation.metadata().get("working_directory"),
            Some(&deployment_root.join("env/dev").display().to_string())
        );

        Ok(())
    }

    #[test]
    fn prepare_rejects_missing_deployment_root() {
        let run_context =
            RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-backend-0002"));
        let scenario = BackendRehearsalScenario::new(BackendRehearsalScenarioConfig::new(
            "/definitely/not/a/real/scenario",
        ));

        let error = scenario
            .prepare(&run_context, &StepRunner::new())
            .expect_err("missing deployment root should fail");

        assert!(error.to_string().contains("deployment root does not exist"));
    }

    #[test]
    fn discover_maps_lifecycle_rehearsal_into_named_verification_target() {
        let scenario =
            BackendRehearsalScenario::new(BackendRehearsalScenarioConfig::new("/tmp/scenario"));
        let run_context =
            RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-backend-0003"));
        let request = BackendRequest::new("/tmp/scenario");
        let session = BackendSession::new(&run_context, "terraform", &request);
        let deployment = ScenarioDeployment::new("terraform", session, BackendOutputs::new());

        let discovery = scenario
            .discover(&deployment, &StepRunner::new())
            .expect("discovery should succeed");

        assert_eq!(discovery.cleanup_actions().len(), 0);
        assert_eq!(
            discovery.verification().target(),
            &ScenarioTarget::NamedOutput {
                key: "lifecycle".to_string(),
                value: "applied".to_string()
            }
        );
    }

    #[test]
    fn prepare_does_not_require_provider_specific_inputs() -> io::Result<()> {
        let temp_dir = TestDir::new("backend-rehearsal-tests", "prepare-generic")?;
        let deployment_root = temp_dir.path().join("deployment");
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-backend-0004"));
        fs::create_dir_all(&deployment_root)?;
        run_context.materialize()?;

        let scenario =
            BackendRehearsalScenario::new(BackendRehearsalScenarioConfig::new(&deployment_root));

        let preparation = scenario
            .prepare(&run_context, &StepRunner::new())
            .map_err(io::Error::other)?;

        assert_eq!(
            preparation.backend_request().deployment_root(),
            Path::new(&deployment_root)
        );
        assert!(preparation.backend_request().environment().is_empty());

        Ok(())
    }
}
