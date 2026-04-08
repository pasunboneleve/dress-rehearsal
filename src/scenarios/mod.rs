//! Scenario abstractions and concrete rehearsal scenarios belong here.

use crate::backends::{BackendOutputs, BackendRequest, BackendSession};
use crate::cleanup::CleanupAction;
use crate::context::RunContext;
use crate::steps::StepRunner;
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Scenario-local input materialized before backend initialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioPreparation {
    backend_request: BackendRequest,
    cleanup_actions: Vec<CleanupAction>,
    metadata: BTreeMap<String, String>,
}

impl ScenarioPreparation {
    pub fn new(backend_request: BackendRequest) -> Self {
        Self {
            backend_request,
            cleanup_actions: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn backend_request(&self) -> &BackendRequest {
        &self.backend_request
    }

    pub fn cleanup_actions(&self) -> &[CleanupAction] {
        &self.cleanup_actions
    }

    pub fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }

    pub fn with_cleanup_action(mut self, action: CleanupAction) -> Self {
        self.cleanup_actions.push(action);
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Deployment state handed back to the scenario after a backend run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioDeployment {
    backend_name: String,
    session: BackendSession,
    outputs: BackendOutputs,
}

impl ScenarioDeployment {
    pub fn new(
        backend_name: impl Into<String>,
        session: BackendSession,
        outputs: BackendOutputs,
    ) -> Self {
        Self {
            backend_name: backend_name.into(),
            session,
            outputs,
        }
    }

    pub fn backend_name(&self) -> &str {
        &self.backend_name
    }

    pub fn session(&self) -> &BackendSession {
        &self.session
    }

    pub fn outputs(&self) -> &BackendOutputs {
        &self.outputs
    }
}

/// Scenario-owned verification wiring. The verification subsystem can later
/// translate this into a concrete `VerificationSpec`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioVerification {
    readiness_label: String,
    target: ScenarioTarget,
    metadata: BTreeMap<String, String>,
}

impl ScenarioVerification {
    pub fn new(readiness_label: impl Into<String>, target: ScenarioTarget) -> Self {
        Self {
            readiness_label: readiness_label.into(),
            target,
            metadata: BTreeMap::new(),
        }
    }

    pub fn readiness_label(&self) -> &str {
        &self.readiness_label
    }

    pub fn target(&self) -> &ScenarioTarget {
        &self.target
    }

    pub fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScenarioTarget {
    HttpEndpoint { url: String },
    NamedOutput { key: String, value: String },
}

/// Scenario-owned discovery result after a backend deploy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScenarioDiscovery {
    verification: ScenarioVerification,
    cleanup_actions: Vec<CleanupAction>,
    surfaced_values: BTreeMap<String, String>,
}

impl ScenarioDiscovery {
    pub fn new(verification: ScenarioVerification) -> Self {
        Self {
            verification,
            cleanup_actions: Vec::new(),
            surfaced_values: BTreeMap::new(),
        }
    }

    pub fn verification(&self) -> &ScenarioVerification {
        &self.verification
    }

    pub fn cleanup_actions(&self) -> &[CleanupAction] {
        &self.cleanup_actions
    }

    pub fn surfaced_values(&self) -> &BTreeMap<String, String> {
        &self.surfaced_values
    }

    pub fn with_cleanup_action(mut self, action: CleanupAction) -> Self {
        self.cleanup_actions.push(action);
        self
    }

    pub fn with_surfaced_value(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.surfaced_values.insert(key.into(), value.into());
        self
    }
}

pub trait Scenario {
    fn name(&self) -> &'static str;

    fn prepare(
        &self,
        run_context: &RunContext,
        runner: &StepRunner,
    ) -> Result<ScenarioPreparation, ScenarioError>;

    fn discover(
        &self,
        deployment: &ScenarioDeployment,
        runner: &StepRunner,
    ) -> Result<ScenarioDiscovery, ScenarioError>;
}

#[derive(Debug)]
pub enum ScenarioError {
    InvalidConfiguration {
        scenario_name: String,
        message: String,
    },
    MissingOutput {
        scenario_name: String,
        output_key: String,
    },
    Io {
        scenario_name: String,
        operation: &'static str,
        source: io::Error,
    },
}

impl ScenarioError {
    pub fn invalid_configuration(
        scenario_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::InvalidConfiguration {
            scenario_name: scenario_name.into(),
            message: message.into(),
        }
    }

    pub fn missing_output(scenario_name: impl Into<String>, output_key: impl Into<String>) -> Self {
        Self::MissingOutput {
            scenario_name: scenario_name.into(),
            output_key: output_key.into(),
        }
    }

    pub fn io(
        scenario_name: impl Into<String>,
        operation: &'static str,
        source: io::Error,
    ) -> Self {
        Self::Io {
            scenario_name: scenario_name.into(),
            operation,
            source,
        }
    }
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration {
                scenario_name,
                message,
            } => write!(f, "scenario `{scenario_name}` is invalid: {message}"),
            Self::MissingOutput {
                scenario_name,
                output_key,
            } => write!(
                f,
                "scenario `{scenario_name}` expected backend output `{output_key}`"
            ),
            Self::Io {
                scenario_name,
                operation,
                source,
            } => write!(
                f,
                "scenario `{scenario_name}` failed during {operation}: {source}"
            ),
        }
    }
}

impl std::error::Error for ScenarioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidConfiguration { .. } => None,
            Self::MissingOutput { .. } => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

pub fn require_output<'a>(
    outputs: &'a BackendOutputs,
    scenario_name: &str,
    key: &str,
) -> Result<&'a str, ScenarioError> {
    outputs
        .get(key)
        .ok_or_else(|| ScenarioError::missing_output(scenario_name, key))
}

pub fn scenario_file(
    run_context: &RunContext,
    relative_path: impl AsRef<Path>,
) -> Result<PathBuf, ScenarioError> {
    let relative_path = relative_path.as_ref();
    if !relative_path.is_relative() {
        return Err(ScenarioError::invalid_configuration(
            "scenario workspace",
            "scenario paths must remain relative to the scenario workspace",
        ));
    }
    if relative_path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ScenarioError::invalid_configuration(
            "scenario workspace",
            "scenario paths must not traverse outside the scenario workspace",
        ));
    }

    Ok(run_context.work_dir().join("scenarios").join(relative_path))
}

#[cfg(test)]
mod tests {
    use super::{
        Scenario, ScenarioDeployment, ScenarioDiscovery, ScenarioPreparation, ScenarioTarget,
        ScenarioVerification, require_output, scenario_file,
    };
    use crate::backends::{BackendOutputs, BackendRequest, BackendSession};
    use crate::cleanup::CleanupAction;
    use crate::context::{RunContext, RunId};
    use crate::steps::{StepCommand, StepRunner};
    use std::path::PathBuf;

    struct FakeScenario;

    impl Scenario for FakeScenario {
        fn name(&self) -> &'static str {
            "fake-scenario"
        }

        fn prepare(
            &self,
            _run_context: &RunContext,
            _runner: &StepRunner,
        ) -> Result<ScenarioPreparation, super::ScenarioError> {
            Ok(ScenarioPreparation::new(
                BackendRequest::new("/tmp/scenario-root")
                    .with_working_directory("/tmp/scenario-root/work"),
            )
            .with_cleanup_action(CleanupAction::new(
                "cleanup-workspace",
                StepCommand::new("cleanup-step", "/bin/true"),
            ))
            .with_metadata("service", "fake"))
        }

        fn discover(
            &self,
            deployment: &ScenarioDeployment,
            _runner: &StepRunner,
        ) -> Result<ScenarioDiscovery, super::ScenarioError> {
            let url = require_output(deployment.outputs(), self.name(), "service_url")?;
            Ok(ScenarioDiscovery::new(ScenarioVerification::new(
                "service ready",
                ScenarioTarget::HttpEndpoint {
                    url: url.to_string(),
                },
            ))
            .with_cleanup_action(CleanupAction::new(
                "cleanup-service",
                StepCommand::new("cleanup-step", "/bin/true"),
            ))
            .with_surfaced_value("service_url", url))
        }
    }

    #[test]
    fn preparation_keeps_backend_request_and_cleanup_expectations() {
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-3001"));
        let scenario = FakeScenario;

        let preparation = scenario
            .prepare(&run_context, &StepRunner::new())
            .expect("scenario should prepare");

        assert_eq!(
            preparation.backend_request().deployment_root(),
            PathBuf::from("/tmp/scenario-root")
        );
        assert_eq!(preparation.cleanup_actions().len(), 1);
        assert_eq!(
            preparation.metadata().get("service"),
            Some(&"fake".to_string())
        );
    }

    #[test]
    fn discovery_surfaces_verification_and_cleanup_hooks() {
        let scenario = FakeScenario;
        let mut outputs = BackendOutputs::new();
        outputs.insert("service_url", "https://example.test");
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-3002"));
        let request = BackendRequest::new("/tmp/scenario-root");
        let session = BackendSession::new(&run_context, "terraform", &request);
        let deployment = ScenarioDeployment::new("terraform", session, outputs);

        let discovery = scenario
            .discover(&deployment, &StepRunner::new())
            .expect("scenario should discover verification data");

        assert_eq!(discovery.cleanup_actions().len(), 1);
        assert_eq!(
            discovery.surfaced_values().get("service_url"),
            Some(&"https://example.test".to_string())
        );
        assert_eq!(discovery.verification().readiness_label(), "service ready");
        assert_eq!(
            discovery.verification().target(),
            &ScenarioTarget::HttpEndpoint {
                url: "https://example.test".to_string()
            }
        );
    }

    #[test]
    fn missing_outputs_fail_explicitly() {
        let scenario = FakeScenario;
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-3003"));
        let request = BackendRequest::new("/tmp/scenario-root");
        let session = BackendSession::new(&run_context, "terraform", &request);
        let deployment = ScenarioDeployment::new("terraform", session, BackendOutputs::new());
        let error = scenario
            .discover(&deployment, &StepRunner::new())
            .expect_err("missing outputs should fail");

        assert!(error.to_string().contains("service_url"));
    }

    #[test]
    fn scenario_files_live_under_run_work_directory() {
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-3004"));

        let path = scenario_file(&run_context, "ecs/service.json").expect("path should resolve");

        assert_eq!(
            path,
            PathBuf::from("/tmp/dress-runs/run-fixed-3004/work/scenarios/ecs/service.json")
        );
    }

    #[test]
    fn scenario_file_rejects_absolute_paths() {
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-3005"));
        let error = scenario_file(&run_context, "/tmp/escape")
            .expect_err("absolute path should be rejected");

        assert!(
            error
                .to_string()
                .contains("scenario paths must remain relative"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn scenario_file_rejects_parent_traversal() {
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-3006"));
        let error = scenario_file(&run_context, "../escape")
            .expect_err("parent traversal should be rejected");

        assert!(
            error
                .to_string()
                .contains("scenario paths must not traverse outside"),
            "unexpected error: {error}"
        );
    }
}
