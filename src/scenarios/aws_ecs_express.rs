use crate::backends::BackendRequest;
use crate::context::RunContext;
use crate::scenarios::{
    Scenario, ScenarioDeployment, ScenarioDiscovery, ScenarioError, ScenarioPreparation,
    ScenarioTarget, ScenarioVerification,
};
use crate::steps::{StepCommand, StepRunner};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwsEcsExpressScenarioConfig {
    deployment_root: PathBuf,
    working_directory: Option<PathBuf>,
    region: String,
    aws_cli_program: PathBuf,
}

impl AwsEcsExpressScenarioConfig {
    pub fn new(deployment_root: impl Into<PathBuf>, region: impl Into<String>) -> Self {
        Self {
            deployment_root: deployment_root.into(),
            working_directory: None,
            region: region.into(),
            aws_cli_program: PathBuf::from("aws"),
        }
    }

    pub fn deployment_root(&self) -> &Path {
        &self.deployment_root
    }

    pub fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    pub fn aws_cli_program(&self) -> &Path {
        &self.aws_cli_program
    }

    pub fn with_working_directory(mut self, working_directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(working_directory.into());
        self
    }

    pub fn with_aws_cli_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.aws_cli_program = program.into();
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwsEcsExpressScenario {
    config: AwsEcsExpressScenarioConfig,
}

impl AwsEcsExpressScenario {
    pub fn new(config: AwsEcsExpressScenarioConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &AwsEcsExpressScenarioConfig {
        &self.config
    }

    pub fn prerequisite_check(&self) -> StepCommand {
        let aws_check = shell_presence_check(self.config.aws_cli_program());
        StepCommand::new("aws-ecs-prerequisites", "/bin/sh")
            .with_args(["-c".to_string(), aws_check])
    }

    fn backend_request(&self) -> BackendRequest {
        let mut request = BackendRequest::new(self.config.deployment_root())
            .with_env("AWS_REGION", self.config.region())
            .with_env("DRESS_SCENARIO", self.name());

        if let Some(working_directory) = self.config.working_directory() {
            request = request.with_working_directory(working_directory);
        }

        request
    }

    fn preparation_metadata(&self) -> [(String, String); 1] {
        [("region".to_string(), self.config.region().to_string())]
    }
}

impl Scenario for AwsEcsExpressScenario {
    fn name(&self) -> &'static str {
        "aws-ecs-express"
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

        let prerequisite_command = self.prerequisite_check();

        let mut preparation = ScenarioPreparation::new(self.backend_request())
            .with_preparation_step(prerequisite_command.clone())
            .with_metadata("prerequisite_step", prerequisite_command.display_command());

        for (key, value) in self.preparation_metadata() {
            preparation = preparation.with_metadata(key, value);
        }

        Ok(preparation)
    }

    fn discover(
        &self,
        _deployment: &ScenarioDeployment,
        _runner: &StepRunner,
    ) -> Result<ScenarioDiscovery, ScenarioError> {
        Ok(ScenarioDiscovery::new(
            ScenarioVerification::new(
                "apply completed",
                ScenarioTarget::NamedOutput {
                    key: "lifecycle".to_string(),
                    value: "applied".to_string(),
                },
            )
            .with_metadata("region", self.config.region()),
        )
        .with_surfaced_value("region", self.config.region()))
    }
}

fn shell_quote_path(path: &Path) -> String {
    let rendered = path.display().to_string();
    format!("'{}'", rendered.replace('\'', r#"'\''"#))
}

fn shell_presence_check(program: &Path) -> String {
    if program.is_absolute() || program.components().count() > 1 {
        format!("test -x {}", shell_quote_path(program))
    } else {
        format!("command -v {} >/dev/null", shell_quote_path(program))
    }
}

#[cfg(test)]
mod tests {
    use super::{AwsEcsExpressScenario, AwsEcsExpressScenarioConfig};
    use crate::backends::{BackendOutputs, BackendRequest, BackendSession};
    use crate::context::{RunContext, RunId};
    use crate::scenarios::{Scenario, ScenarioDeployment, ScenarioTarget};
    use crate::steps::StepRunner;
    use std::env;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> io::Result<Self> {
            let path = env::temp_dir().join(format!(
                "dress-rehearsal-aws-ecs-tests-{name}-{}",
                RunId::generate().as_str()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn path(&self) -> &PathBuf {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn prepare_maps_aws_specific_concerns_into_scenario_boundary() -> io::Result<()> {
        let temp_dir = TestDir::new("prepare")?;
        let deployment_root = temp_dir.path().join("terraform");
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-ecs-0001"));
        fs::create_dir_all(&deployment_root)?;
        run_context.materialize()?;

        let scenario = AwsEcsExpressScenario::new(
            AwsEcsExpressScenarioConfig::new(&deployment_root, "ap-southeast-2")
                .with_working_directory(deployment_root.join("env/dev")),
        );

        let preparation = scenario
            .prepare(&run_context, &StepRunner::new())
            .map_err(io::Error::other)?;

        assert_eq!(
            preparation.backend_request().deployment_root(),
            deployment_root.as_path()
        );
        assert_eq!(
            preparation
                .backend_request()
                .environment()
                .get("AWS_REGION"),
            Some(&"ap-southeast-2".to_string())
        );
        assert_eq!(preparation.preparation_steps().len(), 1);
        assert_eq!(
            preparation.preparation_steps()[0].name().as_str(),
            "aws-ecs-prerequisites"
        );
        assert_eq!(
            preparation.metadata().get("region"),
            Some(&"ap-southeast-2".to_string())
        );
        assert!(preparation.metadata().contains_key("prerequisite_step"));
        assert_eq!(preparation.cleanup_actions().len(), 0);

        Ok(())
    }

    #[test]
    fn prepare_does_not_require_local_task_definition_file() -> io::Result<()> {
        let temp_dir = TestDir::new("prepare-without-task-definition")?;
        let deployment_root = temp_dir.path().join("terraform");
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-ecs-0006"));
        fs::create_dir_all(&deployment_root)?;
        run_context.materialize()?;

        let scenario = AwsEcsExpressScenario::new(AwsEcsExpressScenarioConfig::new(
            &deployment_root,
            "ap-southeast-2",
        ));

        let preparation = scenario
            .prepare(&run_context, &StepRunner::new())
            .map_err(io::Error::other)?;

        assert_eq!(
            preparation.backend_request().deployment_root(),
            Path::new(&deployment_root)
        );
        assert_eq!(preparation.preparation_steps().len(), 1);

        Ok(())
    }

    #[test]
    fn discover_maps_lifecycle_rehearsal_into_named_verification_target() {
        let scenario = AwsEcsExpressScenario::new(AwsEcsExpressScenarioConfig::new(
            "/tmp/scenario",
            "us-east-1",
        ));
        let run_context =
            RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-ecs-0002"));
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
        assert_eq!(
            discovery.verification().metadata().get("region"),
            Some(&"us-east-1".to_string())
        );
        assert_eq!(
            discovery.surfaced_values().get("region"),
            Some(&"us-east-1".to_string())
        );
    }

    #[test]
    fn prepare_rejects_missing_deployment_root() {
        let run_context =
            RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-ecs-0003"));
        let scenario = AwsEcsExpressScenario::new(AwsEcsExpressScenarioConfig::new(
            "/definitely/not/a/real/scenario",
            "us-east-1",
        ));

        let error = scenario
            .prepare(&run_context, &StepRunner::new())
            .expect_err("missing deployment root should fail");

        assert!(error.to_string().contains("deployment root does not exist"));
    }

    #[test]
    fn discover_does_not_require_service_specific_outputs() {
        let scenario = AwsEcsExpressScenario::new(AwsEcsExpressScenarioConfig::new(
            "/tmp/scenario",
            "us-east-1",
        ));
        let run_context =
            RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-ecs-0004"));
        let request = BackendRequest::new("/tmp/scenario");
        let session = BackendSession::new(&run_context, "terraform", &request);
        let deployment = ScenarioDeployment::new("terraform", session, BackendOutputs::new());

        let discovery = scenario
            .discover(&deployment, &StepRunner::new())
            .expect("discovery should succeed");

        assert_eq!(
            discovery.verification().target(),
            &ScenarioTarget::NamedOutput {
                key: "lifecycle".to_string(),
                value: "applied".to_string()
            }
        );
    }
}
