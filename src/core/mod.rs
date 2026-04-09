//! Rehearsal orchestration belongs here.

use crate::backends::{BackendError, DeploymentBackend};
use crate::cleanup::{CleanupManager, CleanupReport};
use crate::context::RunContext;
use crate::scenarios::{Scenario, ScenarioDeployment, ScenarioError};
use crate::steps::{StepError, StepEvent, StepRunner, StepTerminalStatus};
use crate::verification::{
    VerificationReport, VerificationRunError, execute_verification, verification_spec_from_scenario,
};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn rehearse(
    runs_root: impl AsRef<Path>,
    backend: &dyn DeploymentBackend,
    scenario: &dyn Scenario,
    runner: &StepRunner,
) -> RehearsalOutcome {
    let run_context = RunContext::new(runs_root.as_ref());
    if let Err(source) = run_context.materialize() {
        let mut failure = RehearsalFailure {
            run_context,
            stage: RehearsalStage::Materialize,
            error: RehearsalError::Io {
                operation: "materialize run context",
                source,
            },
            cleanup_report: None,
            summary_path: None,
            step_log_path: None,
        };
        write_observability_artifacts(&mut failure, runner);
        return RehearsalOutcome::Failed(failure);
    }

    let mut cleanup_manager = CleanupManager::new();

    let preparation = match scenario.prepare(&run_context, runner) {
        Ok(preparation) => preparation,
        Err(source) => {
            return failed_outcome(
                run_context,
                RehearsalStage::Prepare,
                RehearsalError::Scenario(source),
                cleanup_manager,
                runner,
            );
        }
    };

    for action in preparation.cleanup_actions() {
        cleanup_manager.register(action.clone());
    }

    for command in preparation.preparation_steps() {
        if let Err(source) = runner.run_command(command) {
            return failed_outcome(
                run_context,
                RehearsalStage::Prepare,
                RehearsalError::Step(source),
                cleanup_manager,
                runner,
            );
        }
    }

    let session = match backend.initialize(&run_context, preparation.backend_request(), runner) {
        Ok(session) => session,
        Err(source) => {
            return failed_outcome(
                run_context,
                RehearsalStage::Initialize,
                RehearsalError::Backend(source),
                cleanup_manager,
                runner,
            );
        }
    };
    cleanup_manager.register(backend.destroy_action(&session));

    if let Err(source) = backend.deploy(&session, runner) {
        return failed_outcome(
            run_context,
            RehearsalStage::Deploy,
            RehearsalError::Backend(source),
            cleanup_manager,
            runner,
        );
    }

    let outputs = match backend.outputs(&session, runner) {
        Ok(outputs) => outputs,
        Err(source) => {
            return failed_outcome(
                run_context,
                RehearsalStage::Outputs,
                RehearsalError::Backend(source),
                cleanup_manager,
                runner,
            );
        }
    };
    let deployment = ScenarioDeployment::new(backend.name(), session.clone(), outputs);

    let discovery = match scenario.discover(&deployment, runner) {
        Ok(discovery) => discovery,
        Err(source) => {
            return failed_outcome(
                run_context,
                RehearsalStage::Discover,
                RehearsalError::Scenario(source),
                cleanup_manager,
                runner,
            );
        }
    };

    for action in discovery.cleanup_actions() {
        cleanup_manager.register(action.clone());
    }

    let verification_spec = verification_spec_from_scenario(discovery.verification());
    let verification_report = match execute_verification(&verification_spec, runner, &run_context) {
        Ok(report) => report,
        Err(source) => {
            return failed_outcome(
                run_context,
                RehearsalStage::Verify,
                RehearsalError::Verification(source),
                cleanup_manager,
                runner,
            );
        }
    };

    let cleanup_report = cleanup_manager.execute_teardown(runner);
    if cleanup_report.has_failures() {
        let mut failure = RehearsalFailure {
            run_context,
            stage: RehearsalStage::Teardown,
            error: RehearsalError::CleanupFailed,
            cleanup_report: Some(cleanup_report),
            summary_path: None,
            step_log_path: None,
        };
        write_observability_artifacts(&mut failure, runner);
        return RehearsalOutcome::Failed(failure);
    }

    let mut success = RehearsalSuccess {
        run_context,
        verification_report,
        cleanup_report,
        surfaced_values: discovery.surfaced_values().clone(),
        summary_path: None,
        step_log_path: None,
    };
    write_observability_artifacts(&mut success, runner);
    RehearsalOutcome::Succeeded(success)
}

fn failed_outcome(
    run_context: RunContext,
    stage: RehearsalStage,
    error: RehearsalError,
    cleanup_manager: CleanupManager,
    runner: &StepRunner,
) -> RehearsalOutcome {
    let cleanup_report = cleanup_manager.execute_failure_cleanup(runner, &run_context);
    let mut failure = RehearsalFailure {
        run_context,
        stage,
        error,
        cleanup_report: Some(cleanup_report),
        summary_path: None,
        step_log_path: None,
    };
    write_observability_artifacts(&mut failure, runner);
    RehearsalOutcome::Failed(failure)
}

#[derive(Debug)]
pub enum RehearsalOutcome {
    Succeeded(RehearsalSuccess),
    Failed(RehearsalFailure),
}

#[derive(Debug)]
pub struct RehearsalSuccess {
    run_context: RunContext,
    verification_report: VerificationReport,
    cleanup_report: CleanupReport,
    surfaced_values: BTreeMap<String, String>,
    summary_path: Option<PathBuf>,
    step_log_path: Option<PathBuf>,
}

impl RehearsalSuccess {
    pub fn run_context(&self) -> &RunContext {
        &self.run_context
    }

    pub fn verification_report(&self) -> &VerificationReport {
        &self.verification_report
    }

    pub fn cleanup_report(&self) -> &CleanupReport {
        &self.cleanup_report
    }

    pub fn surfaced_values(&self) -> &BTreeMap<String, String> {
        &self.surfaced_values
    }

    pub fn summary_path(&self) -> Option<&Path> {
        self.summary_path.as_deref()
    }

    pub fn step_log_path(&self) -> Option<&Path> {
        self.step_log_path.as_deref()
    }
}

#[derive(Debug)]
pub struct RehearsalFailure {
    run_context: RunContext,
    stage: RehearsalStage,
    error: RehearsalError,
    cleanup_report: Option<CleanupReport>,
    summary_path: Option<PathBuf>,
    step_log_path: Option<PathBuf>,
}

impl RehearsalFailure {
    pub fn run_context(&self) -> &RunContext {
        &self.run_context
    }

    pub fn stage(&self) -> RehearsalStage {
        self.stage
    }

    pub fn error(&self) -> &RehearsalError {
        &self.error
    }

    pub fn cleanup_report(&self) -> Option<&CleanupReport> {
        self.cleanup_report.as_ref()
    }

    pub fn summary_path(&self) -> Option<&Path> {
        self.summary_path.as_deref()
    }

    pub fn step_log_path(&self) -> Option<&Path> {
        self.step_log_path.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RehearsalStage {
    Materialize,
    Prepare,
    Initialize,
    Deploy,
    Outputs,
    Discover,
    Verify,
    Teardown,
}

#[derive(Debug)]
pub enum RehearsalError {
    Io {
        operation: &'static str,
        source: io::Error,
    },
    Step(StepError),
    Backend(BackendError),
    Scenario(ScenarioError),
    Verification(VerificationRunError),
    CleanupFailed,
}

impl fmt::Display for RehearsalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(f, "{operation} failed: {source}"),
            Self::Step(source) => write!(f, "{source}"),
            Self::Backend(source) => write!(f, "{source}"),
            Self::Scenario(source) => write!(f, "{source}"),
            Self::Verification(source) => write!(f, "{source}"),
            Self::CleanupFailed => f.write_str("cleanup failed after a successful rehearsal"),
        }
    }
}

impl std::error::Error for RehearsalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Step(source) => Some(source),
            Self::Backend(source) => Some(source),
            Self::Scenario(source) => Some(source),
            Self::Verification(source) => Some(source),
            Self::CleanupFailed => None,
        }
    }
}

trait ObservableOutcome {
    fn run_context(&self) -> &RunContext;
    fn render_summary(&self) -> String;
    fn set_summary_path(&mut self, path: PathBuf);
    fn set_step_log_path(&mut self, path: PathBuf);
}

impl ObservableOutcome for RehearsalSuccess {
    fn run_context(&self) -> &RunContext {
        &self.run_context
    }

    fn render_summary(&self) -> String {
        let surfaced_values = render_key_values(self.surfaced_values());
        format!(
            "status=succeeded\nrun_id={}\nroot_dir={}\nartifacts_dir={}\npreserved_dir={}\nsummary_path={}\nstep_log_path={}\nverification_failures=0\nsurfaced_values={}\n",
            self.run_context.run_id(),
            self.run_context.root_dir().display(),
            self.run_context.artifacts_dir().display(),
            self.run_context.preserved_dir().display(),
            self.run_context
                .artifact_path("run/rehearsal-summary.txt")
                .display(),
            self.run_context
                .artifact_path("run/step-events.log")
                .display(),
            surfaced_values,
        )
    }

    fn set_summary_path(&mut self, path: PathBuf) {
        self.summary_path = Some(path);
    }

    fn set_step_log_path(&mut self, path: PathBuf) {
        self.step_log_path = Some(path);
    }
}

impl ObservableOutcome for RehearsalFailure {
    fn run_context(&self) -> &RunContext {
        &self.run_context
    }

    fn render_summary(&self) -> String {
        let cleanup_report = self.cleanup_report.as_ref();
        let recovery_hints = cleanup_report
            .map(|report| {
                report
                    .recovery_hints()
                    .iter()
                    .map(|hint| format!("{}: {}", hint.action_name, hint.hint))
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default();
        let preserved_artifacts = cleanup_report
            .map(|report| {
                report
                    .preserved_artifacts()
                    .iter()
                    .map(|artifact| artifact.preserved_path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .unwrap_or_default();
        let cleanup_failures = cleanup_report
            .map(|report| report.has_failures())
            .unwrap_or(false);
        format!(
            "status=failed\nrun_id={}\nstage={}\nerror={}\nroot_dir={}\nartifacts_dir={}\npreserved_dir={}\nsummary_path={}\nstep_log_path={}\ncleanup_failures={cleanup_failures}\npreserved_artifacts={preserved_artifacts}\nrecovery_hints={recovery_hints}\n",
            self.run_context.run_id(),
            self.stage,
            self.error,
            self.run_context.root_dir().display(),
            self.run_context.artifacts_dir().display(),
            self.run_context.preserved_dir().display(),
            self.run_context
                .artifact_path("run/rehearsal-summary.txt")
                .display(),
            self.run_context
                .artifact_path("run/step-events.log")
                .display(),
        )
    }

    fn set_summary_path(&mut self, path: PathBuf) {
        self.summary_path = Some(path);
    }

    fn set_step_log_path(&mut self, path: PathBuf) {
        self.step_log_path = Some(path);
    }
}

fn write_observability_artifacts(outcome: &mut dyn ObservableOutcome, runner: &StepRunner) {
    let step_log_path = outcome.run_context().artifact_path("run/step-events.log");
    let summary_path = outcome
        .run_context()
        .artifact_path("run/rehearsal-summary.txt");
    let _ = write_step_log(&step_log_path, &runner.recorded_events());
    let _ = write_summary(&summary_path, &outcome.render_summary());
    if step_log_path.is_file() {
        outcome.set_step_log_path(step_log_path);
    }
    if summary_path.is_file() {
        outcome.set_summary_path(summary_path);
    }
}

fn write_step_log(path: &Path, events: &[StepEvent]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut rendered = String::new();
    for event in events {
        rendered.push_str(&render_step_event(event));
        rendered.push('\n');
    }
    fs::write(path, rendered)
}

fn write_summary(path: &Path, summary: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, summary)
}

fn render_step_event(event: &StepEvent) -> String {
    match event {
        StepEvent::Started { step_name, command } => {
            format!("started step={} command={command}", step_name.as_str())
        }
        StepEvent::Finished { step_name, status } => {
            format!(
                "finished step={} status={}",
                step_name.as_str(),
                render_status(status)
            )
        }
    }
}

fn render_status(status: &StepTerminalStatus) -> String {
    match status {
        StepTerminalStatus::Succeeded => "succeeded".to_string(),
        StepTerminalStatus::Failed { exit_code } => match exit_code {
            Some(exit_code) => format!("failed({exit_code})"),
            None => "failed".to_string(),
        },
        StepTerminalStatus::SpawnError { message } => format!("spawn-error({message})"),
    }
}

fn render_key_values(values: &BTreeMap<String, String>) -> String {
    values
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" | ")
}

impl fmt::Display for RehearsalStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Materialize => "materialize",
            Self::Prepare => "prepare",
            Self::Initialize => "initialize",
            Self::Deploy => "deploy",
            Self::Outputs => "outputs",
            Self::Discover => "discover",
            Self::Verify => "verify",
            Self::Teardown => "teardown",
        };
        f.write_str(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{RehearsalOutcome, RehearsalStage, rehearse};
    use crate::backends::{
        BackendError, BackendOutputs, BackendRequest, BackendSession, DeploymentBackend,
    };
    use crate::cleanup::CleanupAction;
    use crate::context::{RunContext, RunId};
    use crate::scenarios::{
        Scenario, ScenarioDeployment, ScenarioDiscovery, ScenarioError, ScenarioPreparation,
        ScenarioTarget, ScenarioVerification,
    };
    use crate::steps::{StepCommand, StepRunner};
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
                "dress-rehearsal-core-tests-{name}-{}",
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

    struct FakeBackend {
        deployment_root: PathBuf,
        cleanup_log_path: PathBuf,
    }

    impl DeploymentBackend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake-backend"
        }

        fn initialize(
            &self,
            run_context: &RunContext,
            request: &BackendRequest,
            _runner: &StepRunner,
        ) -> Result<BackendSession, BackendError> {
            Ok(BackendSession::new(run_context, self.name(), request))
        }

        fn deploy(
            &self,
            _session: &BackendSession,
            _runner: &StepRunner,
        ) -> Result<(), BackendError> {
            Ok(())
        }

        fn outputs(
            &self,
            _session: &BackendSession,
            _runner: &StepRunner,
        ) -> Result<BackendOutputs, BackendError> {
            let mut outputs = BackendOutputs::new();
            outputs.insert(
                "deployment_root",
                self.deployment_root.display().to_string(),
            );
            Ok(outputs)
        }

        fn destroy_action(&self, _session: &BackendSession) -> CleanupAction {
            CleanupAction::new(
                "backend-destroy",
                StepCommand::new("backend-destroy-step", "/bin/sh").with_args([
                    "-c".to_string(),
                    format!(
                        "printf 'backend-destroy\n' >> '{}'",
                        self.cleanup_log_path.display()
                    ),
                ]),
            )
        }

        fn destroy(
            &self,
            _session: &BackendSession,
            _runner: &StepRunner,
        ) -> Result<(), BackendError> {
            Ok(())
        }
    }

    struct FakeScenario {
        deployment_root: PathBuf,
        cleanup_log_path: PathBuf,
        failure_artifact_path: PathBuf,
        should_fail_prepare: bool,
    }

    impl Scenario for FakeScenario {
        fn name(&self) -> &'static str {
            "fake-scenario"
        }

        fn prepare(
            &self,
            _run_context: &RunContext,
            _runner: &StepRunner,
        ) -> Result<ScenarioPreparation, ScenarioError> {
            let preparation = ScenarioPreparation::new(BackendRequest::new(&self.deployment_root))
                .with_preparation_step(
                    StepCommand::new("prepare-step", "/bin/sh")
                        .with_args(["-c".to_string(), "printf 'prepared' >/dev/null".to_string()]),
                )
                .with_cleanup_action(
                    CleanupAction::new(
                        "scenario-cleanup",
                        StepCommand::new("scenario-cleanup-step", "/bin/sh").with_args([
                            "-c".to_string(),
                            format!(
                                "printf 'scenario-cleanup\n' >> '{}'",
                                self.cleanup_log_path.display()
                            ),
                        ]),
                    )
                    .preserve_on_failure(crate::cleanup::CleanupArtifact::new(
                        &self.failure_artifact_path,
                        "cleanup/failure-artifact.txt",
                    )),
                );

            if self.should_fail_prepare {
                return Ok(preparation.with_preparation_step(
                    StepCommand::new("prepare-step", "/bin/sh")
                        .with_args(["-c".to_string(), "exit 23".to_string()]),
                ));
            }

            Ok(preparation)
        }

        fn discover(
            &self,
            _deployment: &ScenarioDeployment,
            _runner: &StepRunner,
        ) -> Result<ScenarioDiscovery, ScenarioError> {
            Ok(ScenarioDiscovery::new(ScenarioVerification::new(
                "ready",
                ScenarioTarget::NamedOutput {
                    key: "service".to_string(),
                    value: "ok".to_string(),
                },
            ))
            .with_cleanup_action(CleanupAction::new(
                "discovery-cleanup",
                StepCommand::new("discovery-cleanup-step", "/bin/sh").with_args([
                    "-c".to_string(),
                    format!(
                        "printf 'discovery-cleanup\n' >> '{}'",
                        self.cleanup_log_path.display()
                    ),
                ]),
            ))
            .with_surfaced_value("service", "ok"))
        }
    }

    #[test]
    fn rehearses_successful_path_and_tears_down_in_reverse_order() -> io::Result<()> {
        let temp_dir = TestDir::new("success")?;
        let cleanup_log_path = temp_dir.path().join("cleanup.log");
        let deployment_root = temp_dir.path().join("deployment");
        fs::create_dir_all(&deployment_root)?;
        let backend = FakeBackend {
            deployment_root: deployment_root.clone(),
            cleanup_log_path: cleanup_log_path.clone(),
        };
        let artifact_path = temp_dir.path().join("artifact.txt");
        fs::write(&artifact_path, "artifact")?;
        let scenario = FakeScenario {
            deployment_root,
            cleanup_log_path: cleanup_log_path.clone(),
            failure_artifact_path: artifact_path,
            should_fail_prepare: false,
        };

        let outcome = rehearse(temp_dir.path(), &backend, &scenario, &StepRunner::new());

        match outcome {
            RehearsalOutcome::Succeeded(success) => {
                assert!(success.verification_report().passed());
                assert!(success.cleanup_report().results().len() >= 2);
                assert_eq!(
                    success.surfaced_values().get("service"),
                    Some(&"ok".to_string())
                );

                let cleanup_log = fs::read_to_string(cleanup_log_path)?;
                assert_eq!(
                    cleanup_log,
                    "discovery-cleanup\nbackend-destroy\nscenario-cleanup\n"
                );
                let summary_path = success
                    .summary_path()
                    .expect("summary path should be recorded");
                let step_log_path = success
                    .step_log_path()
                    .expect("step log path should be recorded");
                let summary = fs::read_to_string(summary_path)?;
                let step_log = fs::read_to_string(step_log_path)?;
                assert!(summary.contains("status=succeeded"));
                assert!(summary.contains("artifacts_dir="));
                assert!(summary.contains("step_log_path="));
                assert!(summary.contains("surfaced_values=service=ok"));
                assert!(step_log.contains("started step=prepare-step"));
                assert!(step_log.contains("finished step=backend-destroy-step status=succeeded"));
            }
            RehearsalOutcome::Failed(failure) => {
                panic!(
                    "expected success, got failure at {:?}: {}",
                    failure.stage(),
                    failure.error()
                )
            }
        }

        Ok(())
    }

    #[test]
    fn failed_preparation_runs_failure_cleanup_and_preserves_artifacts() -> io::Result<()> {
        let temp_dir = TestDir::new("failure-cleanup")?;
        let cleanup_log_path = temp_dir.path().join("cleanup.log");
        let deployment_root = temp_dir.path().join("deployment");
        fs::create_dir_all(&deployment_root)?;
        let backend = FakeBackend {
            deployment_root: deployment_root.clone(),
            cleanup_log_path,
        };
        let artifact_path = temp_dir.path().join("artifact.txt");
        fs::write(&artifact_path, "artifact")?;
        let scenario = FakeScenario {
            deployment_root,
            cleanup_log_path: temp_dir.path().join("cleanup.log"),
            failure_artifact_path: artifact_path,
            should_fail_prepare: true,
        };

        let outcome = rehearse(temp_dir.path(), &backend, &scenario, &StepRunner::new());

        match outcome {
            RehearsalOutcome::Failed(failure) => {
                assert_eq!(failure.stage(), RehearsalStage::Prepare);
                let cleanup_report = failure
                    .cleanup_report()
                    .expect("failed rehearsals should include cleanup");
                assert!(!cleanup_report.results().is_empty());
                assert!(cleanup_report.preserved_artifacts().iter().any(|artifact| {
                    artifact
                        .preserved_path
                        .ends_with("cleanup/failure-artifact.txt")
                }));
                assert!(
                    failure
                        .run_context()
                        .preserved_dir()
                        .join("cleanup/failure-artifact.txt")
                        .is_file()
                );
                let summary_path = failure
                    .summary_path()
                    .expect("summary path should be recorded");
                let step_log_path = failure
                    .step_log_path()
                    .expect("step log path should be recorded");
                let summary = fs::read_to_string(summary_path)?;
                let step_log = fs::read_to_string(step_log_path)?;
                assert!(summary.contains("status=failed"));
                assert!(summary.contains("stage=prepare"));
                assert!(summary.contains("preserved_artifacts="));
                assert!(summary.contains("step_log_path="));
                assert!(step_log.contains("finished step=prepare-step status=failed(23)"));
            }
            RehearsalOutcome::Succeeded(_) => panic!("expected failure outcome"),
        }

        Ok(())
    }
}
