use crate::backends::BackendRequest;
use crate::cleanup::{CleanupAction, CleanupArtifact};
use crate::context::RunContext;
use crate::scenarios::{
    Scenario, ScenarioDeployment, ScenarioDiscovery, ScenarioError, ScenarioPreparation,
    ScenarioTarget, ScenarioVerification, require_output, scenario_file,
};
use crate::steps::{StepCommand, StepRunner};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AwsEcsExpressScenarioConfig {
    deployment_root: PathBuf,
    working_directory: Option<PathBuf>,
    cluster_output_key: String,
    service_output_key: String,
    url_output_key: String,
    region: String,
    expected_health_path: String,
    task_definition_path: PathBuf,
}

impl AwsEcsExpressScenarioConfig {
    pub fn new(
        deployment_root: impl Into<PathBuf>,
        region: impl Into<String>,
        expected_health_path: impl Into<String>,
    ) -> Self {
        Self {
            deployment_root: deployment_root.into(),
            working_directory: None,
            cluster_output_key: "cluster_name".to_string(),
            service_output_key: "service_name".to_string(),
            url_output_key: "service_url".to_string(),
            region: region.into(),
            expected_health_path: expected_health_path.into(),
            task_definition_path: PathBuf::from("app/task-definition.json"),
        }
    }

    pub fn deployment_root(&self) -> &Path {
        &self.deployment_root
    }

    pub fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    pub fn cluster_output_key(&self) -> &str {
        &self.cluster_output_key
    }

    pub fn service_output_key(&self) -> &str {
        &self.service_output_key
    }

    pub fn url_output_key(&self) -> &str {
        &self.url_output_key
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    pub fn expected_health_path(&self) -> &str {
        &self.expected_health_path
    }

    pub fn task_definition_path(&self) -> &Path {
        &self.task_definition_path
    }

    pub fn with_working_directory(mut self, working_directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(working_directory.into());
        self
    }

    pub fn with_cluster_output_key(mut self, key: impl Into<String>) -> Self {
        self.cluster_output_key = key.into();
        self
    }

    pub fn with_service_output_key(mut self, key: impl Into<String>) -> Self {
        self.service_output_key = key.into();
        self
    }

    pub fn with_url_output_key(mut self, key: impl Into<String>) -> Self {
        self.url_output_key = key.into();
        self
    }

    pub fn with_task_definition_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.task_definition_path = path.into();
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
        StepCommand::new("aws-ecs-prerequisites", "/bin/sh").with_args([
            "-c".to_string(),
            "command -v aws >/dev/null && command -v docker >/dev/null".to_string(),
        ])
    }

    pub fn bootstrap_check(&self, run_context: &RunContext) -> Result<StepCommand, ScenarioError> {
        let task_definition = scenario_file(run_context, self.config.task_definition_path())?;
        Ok(StepCommand::new("aws-ecs-bootstrap", "/bin/sh").with_args([
            "-c".to_string(),
            format!("test -f {}", shell_quote(&task_definition)),
        ]))
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

    fn preparation_metadata(&self) -> [(String, String); 4] {
        [
            ("region".to_string(), self.config.region().to_string()),
            (
                "health_path".to_string(),
                self.config.expected_health_path().to_string(),
            ),
            (
                "cluster_output_key".to_string(),
                self.config.cluster_output_key().to_string(),
            ),
            (
                "service_output_key".to_string(),
                self.config.service_output_key().to_string(),
            ),
        ]
    }

    fn terraform_destroy_cleanup(&self) -> CleanupAction {
        let working_directory = self
            .config
            .working_directory()
            .unwrap_or(self.config.deployment_root());

        CleanupAction::new(
            "aws-ecs-terraform-destroy",
            StepCommand::new("terraform-destroy-step", "/bin/sh")
                .with_args(["-c".to_string(), "terraform destroy -auto-approve".to_string()])
                .with_current_dir(working_directory),
        )
        .recovery_hint(
            "If cleanup fails, inspect the ECS service and Terraform state before retrying destroy.",
        )
    }

    fn ecs_service_drain_cleanup(&self, cluster_name: &str, service_name: &str) -> CleanupAction {
        CleanupAction::new(
            "aws-ecs-service-drain",
            StepCommand::new("aws-ecs-service-drain-step", "/bin/sh").with_args([
                "-c".to_string(),
                format!(
                    "aws ecs update-service --region {} --cluster {} --service {} --desired-count 0",
                    shell_quote_literal(self.config.region()),
                    shell_quote_literal(cluster_name),
                    shell_quote_literal(service_name),
                ),
            ]),
        )
        .recovery_hint("If the ECS service remains active, scale it to zero and inspect target group health.")
    }
}

impl Scenario for AwsEcsExpressScenario {
    fn name(&self) -> &'static str {
        "aws-ecs-express"
    }

    fn prepare(
        &self,
        run_context: &RunContext,
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
        let bootstrap_command = self.bootstrap_check(run_context)?;
        let task_definition = scenario_file(run_context, self.config.task_definition_path())?;

        let mut preparation = ScenarioPreparation::new(self.backend_request())
            .with_cleanup_action(self.terraform_destroy_cleanup().preserve_on_failure(
                CleanupArtifact::new(&task_definition, "aws-ecs/task-definition.json"),
            ))
            .with_metadata("prerequisite_step", prerequisite_command.display_command())
            .with_metadata("bootstrap_step", bootstrap_command.display_command());

        for (key, value) in self.preparation_metadata() {
            preparation = preparation.with_metadata(key, value);
        }

        Ok(preparation)
    }

    fn discover(
        &self,
        deployment: &ScenarioDeployment,
        _runner: &StepRunner,
    ) -> Result<ScenarioDiscovery, ScenarioError> {
        let cluster_name = require_output(
            deployment.outputs(),
            self.name(),
            self.config.cluster_output_key(),
        )?;
        let service_name = require_output(
            deployment.outputs(),
            self.name(),
            self.config.service_output_key(),
        )?;
        let service_url = require_output(
            deployment.outputs(),
            self.name(),
            self.config.url_output_key(),
        )?;

        Ok(ScenarioDiscovery::new(
            ScenarioVerification::new(
                "ecs service reachable",
                ScenarioTarget::HttpEndpoint {
                    url: join_url_path(service_url, self.config.expected_health_path()),
                },
            )
            .with_metadata("cluster_name", cluster_name)
            .with_metadata("service_name", service_name)
            .with_metadata("region", self.config.region()),
        )
        .with_cleanup_action(self.ecs_service_drain_cleanup(cluster_name, service_name))
        .with_surfaced_value("cluster_name", cluster_name)
        .with_surfaced_value("service_name", service_name)
        .with_surfaced_value("service_url", service_url)
        .with_surfaced_value("region", self.config.region()))
    }
}

fn shell_quote(path: &Path) -> String {
    let rendered = path.display().to_string();
    format!("'{}'", rendered.replace('\'', r#"'\''"#))
}

fn shell_quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn join_url_path(base_url: &str, path: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        base_url.to_string()
    } else {
        format!("{base_url}/{path}")
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
    use std::path::PathBuf;

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
        fs::create_dir_all(run_context.work_dir().join("scenarios").join("app"))?;
        fs::write(
            run_context
                .work_dir()
                .join("scenarios/app/task-definition.json"),
            "{}",
        )?;

        let scenario = AwsEcsExpressScenario::new(
            AwsEcsExpressScenarioConfig::new(&deployment_root, "ap-southeast-2", "/health")
                .with_task_definition_path("app/task-definition.json")
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
        assert_eq!(preparation.cleanup_actions().len(), 1);
        assert_eq!(
            preparation.cleanup_actions()[0].name(),
            "aws-ecs-terraform-destroy"
        );
        assert_eq!(
            preparation.metadata().get("health_path"),
            Some(&"/health".to_string())
        );
        assert!(preparation.metadata().contains_key("prerequisite_step"));
        assert!(preparation.metadata().contains_key("bootstrap_step"));

        Ok(())
    }

    #[test]
    fn discover_maps_backend_outputs_into_http_verification_target() {
        let scenario = AwsEcsExpressScenario::new(AwsEcsExpressScenarioConfig::new(
            "/tmp/scenario",
            "us-east-1",
            "/health",
        ));
        let run_context =
            RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-ecs-0002"));
        let request = BackendRequest::new("/tmp/scenario");
        let session = BackendSession::new(&run_context, "terraform", &request);
        let mut outputs = BackendOutputs::new();
        outputs.insert("cluster_name", "dress-cluster");
        outputs.insert("service_name", "dress-service");
        outputs.insert("service_url", "https://service.example.test");
        let deployment = ScenarioDeployment::new("terraform", session, outputs);

        let discovery = scenario
            .discover(&deployment, &StepRunner::new())
            .expect("discovery should succeed");

        assert_eq!(discovery.cleanup_actions().len(), 1);
        assert_eq!(
            discovery.cleanup_actions()[0].name(),
            "aws-ecs-service-drain"
        );
        assert_eq!(
            discovery.verification().target(),
            &ScenarioTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string()
            }
        );
        assert_eq!(
            discovery.verification().metadata().get("cluster_name"),
            Some(&"dress-cluster".to_string())
        );
    }

    #[test]
    fn prepare_rejects_missing_deployment_root() {
        let run_context =
            RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-ecs-0003"));
        let scenario = AwsEcsExpressScenario::new(AwsEcsExpressScenarioConfig::new(
            "/definitely/not/a/real/scenario",
            "us-east-1",
            "/health",
        ));

        let error = scenario
            .prepare(&run_context, &StepRunner::new())
            .expect_err("missing deployment root should fail");

        assert!(error.to_string().contains("deployment root does not exist"));
    }

    #[test]
    fn discover_requires_service_specific_outputs() {
        let scenario = AwsEcsExpressScenario::new(AwsEcsExpressScenarioConfig::new(
            "/tmp/scenario",
            "us-east-1",
            "/health",
        ));
        let run_context =
            RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-ecs-0004"));
        let request = BackendRequest::new("/tmp/scenario");
        let session = BackendSession::new(&run_context, "terraform", &request);
        let deployment = ScenarioDeployment::new("terraform", session, BackendOutputs::new());

        let error = scenario
            .discover(&deployment, &StepRunner::new())
            .expect_err("missing outputs should fail");

        assert!(error.to_string().contains("cluster_name"));
    }

    #[test]
    fn discover_normalizes_health_url_joining() {
        let scenario = AwsEcsExpressScenario::new(AwsEcsExpressScenarioConfig::new(
            "/tmp/scenario",
            "us-east-1",
            "health",
        ));
        let run_context =
            RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-ecs-0005"));
        let request = BackendRequest::new("/tmp/scenario");
        let session = BackendSession::new(&run_context, "terraform", &request);
        let mut outputs = BackendOutputs::new();
        outputs.insert("cluster_name", "dress-cluster");
        outputs.insert("service_name", "dress-service");
        outputs.insert("service_url", "https://service.example.test/");
        let deployment = ScenarioDeployment::new("terraform", session, outputs);

        let discovery = scenario
            .discover(&deployment, &StepRunner::new())
            .expect("discovery should succeed");

        assert_eq!(
            discovery.verification().target(),
            &ScenarioTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string()
            }
        );
    }
}
