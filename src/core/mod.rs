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
            failed_step_stdout_path: None,
            failed_step_stderr_path: None,
            summary_path: None,
            step_log_path: None,
        };
        write_observability_artifacts(&mut failure, runner);
        return RehearsalOutcome::Failed(failure);
    }

    runner.set_artifact_root(run_context.artifact_path("steps"));

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
            failed_step_stdout_path: None,
            failed_step_stderr_path: None,
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
        failed_step_stdout_path: None,
        failed_step_stderr_path: None,
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
    failed_step_stdout_path: Option<PathBuf>,
    failed_step_stderr_path: Option<PathBuf>,
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

    pub fn failed_step_stdout_path(&self) -> Option<&Path> {
        self.failed_step_stdout_path.as_deref()
    }

    pub fn failed_step_stderr_path(&self) -> Option<&Path> {
        self.failed_step_stderr_path.as_deref()
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
    fn set_failed_step_output_paths(
        &mut self,
        stdout_path: Option<PathBuf>,
        stderr_path: Option<PathBuf>,
    );
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

    fn set_failed_step_output_paths(
        &mut self,
        _stdout_path: Option<PathBuf>,
        _stderr_path: Option<PathBuf>,
    ) {
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
                    .map(|hint| {
                        format!(
                            "{}: {}",
                            hint.action_name,
                            sanitize_summary_value(&hint.hint)
                        )
                    })
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
            "status=failed\nrun_id={}\nstage={}\nerror={}\nroot_dir={}\nartifacts_dir={}\npreserved_dir={}\nsummary_path={}\nstep_log_path={}\nfailed_step_stdout_path={}\nfailed_step_stderr_path={}\ncleanup_failures={cleanup_failures}\npreserved_artifacts={preserved_artifacts}\nrecovery_hints={recovery_hints}\n",
            self.run_context.run_id(),
            self.stage,
            sanitize_summary_value(&self.error.to_string()),
            self.run_context.root_dir().display(),
            self.run_context.artifacts_dir().display(),
            self.run_context.preserved_dir().display(),
            self.run_context
                .artifact_path("run/rehearsal-summary.txt")
                .display(),
            self.run_context
                .artifact_path("run/step-events.log")
                .display(),
            self.failed_step_stdout_path
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            self.failed_step_stderr_path
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        )
    }

    fn set_summary_path(&mut self, path: PathBuf) {
        self.summary_path = Some(path);
    }

    fn set_step_log_path(&mut self, path: PathBuf) {
        self.step_log_path = Some(path);
    }

    fn set_failed_step_output_paths(
        &mut self,
        stdout_path: Option<PathBuf>,
        stderr_path: Option<PathBuf>,
    ) {
        self.failed_step_stdout_path = stdout_path;
        self.failed_step_stderr_path = stderr_path;
    }
}

fn write_observability_artifacts(outcome: &mut dyn ObservableOutcome, runner: &StepRunner) {
    let step_log_path = outcome.run_context().artifact_path("run/step-events.log");
    let summary_path = outcome
        .run_context()
        .artifact_path("run/rehearsal-summary.txt");
    let failed_step = runner
        .recorded_executions()
        .into_iter()
        .rev()
        .find(|execution| execution.status().is_failed());
    outcome.set_failed_step_output_paths(
        failed_step
            .as_ref()
            .and_then(|execution| execution.stdout_path().map(Path::to_path_buf)),
        failed_step
            .as_ref()
            .and_then(|execution| execution.stderr_path().map(Path::to_path_buf)),
    );
    if write_step_log(&step_log_path, &runner.recorded_events()).is_ok() {
        outcome.set_step_log_path(step_log_path);
    }
    if write_summary(&summary_path, &outcome.render_summary()).is_ok() {
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
        StepEvent::Finished {
            step_name,
            status,
            stdout_path,
            stderr_path,
        } => format!(
            "finished step={} status={} stdout_path={} stderr_path={}",
            step_name.as_str(),
            render_status(status),
            stdout_path
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            stderr_path
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        ),
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
        .map(|(key, value)| format!("{key}={}", sanitize_summary_value(value)))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn sanitize_summary_value(value: &str) -> String {
    value.replace('\n', "\\n")
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
    use crate::cleanup::{CleanupAction, CleanupArtifact};
    use crate::context::RunContext;
    use crate::scenarios::{
        Scenario, ScenarioDeployment, ScenarioDiscovery, ScenarioError, ScenarioPreparation,
        ScenarioTarget, ScenarioVerification,
    };
    use crate::steps::{StepCommand, StepRunner};
    use crate::test_support::TestDir;
    use std::fs;
    use std::io;
    use std::path::PathBuf;

    struct FakeBackend {
        deployment_root: PathBuf,
        cleanup_log_path: PathBuf,
        deploy_should_fail: bool,
        destroy_should_fail: bool,
        failure_artifact_path: Option<PathBuf>,
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
            if self.deploy_should_fail {
                return Err(BackendError::io(
                    self.name(),
                    "deploy",
                    io::Error::other("simulated deploy failure"),
                ));
            }
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
            let script = if self.destroy_should_fail {
                format!(
                    "printf 'backend-destroy\\n' >> '{}'; exit 17",
                    self.cleanup_log_path.display()
                )
            } else {
                format!(
                    "printf 'backend-destroy\\n' >> '{}'",
                    self.cleanup_log_path.display()
                )
            };
            let mut action = CleanupAction::new(
                "backend-destroy",
                StepCommand::new("backend-destroy-step", "/bin/sh")
                    .with_args(["-c".to_string(), script]),
            );
            if let Some(path) = &self.failure_artifact_path {
                action = action
                    .preserve_on_failure(CleanupArtifact::new(path, "cleanup/backend-state.txt"));
            }
            if self.destroy_should_fail {
                action =
                    action.recovery_hint("Run the backend destroy command manually for this run.");
            }
            action
        }
    }

    struct FakeScenario {
        deployment_root: PathBuf,
        cleanup_log_path: PathBuf,
        failure_artifact_path: PathBuf,
        verification: ScenarioVerification,
        extra_preparation_steps: Vec<StepCommand>,
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
            let mut preparation =
                ScenarioPreparation::new(BackendRequest::new(&self.deployment_root))
                    .with_preparation_step(
                        StepCommand::new("prepare-step", "/bin/sh").with_args([
                            "-c".to_string(),
                            "printf 'prepared' >/dev/null".to_string(),
                        ]),
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
                        .preserve_on_failure(
                            crate::cleanup::CleanupArtifact::new(
                                &self.failure_artifact_path,
                                "cleanup/failure-artifact.txt",
                            ),
                        ),
                    );
            for step in &self.extra_preparation_steps {
                preparation = preparation.with_preparation_step(step.clone());
            }

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
            Ok(ScenarioDiscovery::new(self.verification.clone())
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
        let temp_dir = TestDir::new("core-tests", "success")?;
        let cleanup_log_path = temp_dir.path().join("cleanup.log");
        let deployment_root = temp_dir.path().join("deployment");
        fs::create_dir_all(&deployment_root)?;
        let backend = FakeBackend {
            deployment_root: deployment_root.clone(),
            cleanup_log_path: cleanup_log_path.clone(),
            deploy_should_fail: false,
            destroy_should_fail: false,
            failure_artifact_path: None,
        };
        let artifact_path = temp_dir.path().join("artifact.txt");
        fs::write(&artifact_path, "artifact")?;
        let scenario = FakeScenario {
            deployment_root,
            cleanup_log_path: cleanup_log_path.clone(),
            failure_artifact_path: artifact_path,
            verification: named_output_verification("ready"),
            extra_preparation_steps: Vec::new(),
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
        let temp_dir = TestDir::new("core-tests", "failure-cleanup")?;
        let cleanup_log_path = temp_dir.path().join("cleanup.log");
        let deployment_root = temp_dir.path().join("deployment");
        fs::create_dir_all(&deployment_root)?;
        let backend = FakeBackend {
            deployment_root: deployment_root.clone(),
            cleanup_log_path,
            deploy_should_fail: false,
            destroy_should_fail: false,
            failure_artifact_path: None,
        };
        let artifact_path = temp_dir.path().join("artifact.txt");
        fs::write(&artifact_path, "artifact")?;
        let scenario = FakeScenario {
            deployment_root,
            cleanup_log_path: temp_dir.path().join("cleanup.log"),
            failure_artifact_path: artifact_path,
            verification: named_output_verification("ready"),
            extra_preparation_steps: Vec::new(),
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

    #[test]
    fn verification_changes_do_not_reshape_cleanup_order() -> io::Result<()> {
        let baseline = successful_rehearsal_with(
            "verification-boundary-baseline",
            named_output_verification("ready"),
            Vec::new(),
        )?;
        let verification_variant = successful_rehearsal_with(
            "verification-boundary-variant",
            named_output_verification("service accepted the rehearsal")
                .with_metadata("probe", "alternate")
                .with_metadata("operator-note", "verification changed only"),
            Vec::new(),
        )?;

        assert_eq!(
            baseline.cleanup_log, verification_variant.cleanup_log,
            "verification-only changes should not alter cleanup order"
        );
        assert_eq!(
            baseline.cleanup_results, verification_variant.cleanup_results,
            "verification-only changes should not register different cleanup actions"
        );

        Ok(())
    }

    #[test]
    fn prepare_failures_do_not_cross_backend_cleanup_boundary() -> io::Result<()> {
        let temp_dir = TestDir::new("core-tests", "prepare-boundary-failure")?;
        let cleanup_log_path = temp_dir.path().join("cleanup.log");
        let deployment_root = temp_dir.path().join("deployment");
        fs::create_dir_all(&deployment_root)?;
        let backend = FakeBackend {
            deployment_root: deployment_root.clone(),
            cleanup_log_path: cleanup_log_path.clone(),
            deploy_should_fail: false,
            destroy_should_fail: false,
            failure_artifact_path: None,
        };
        let artifact_path = temp_dir.path().join("artifact.txt");
        let bootstrap_marker_path = temp_dir.path().join("bootstrap-marker.txt");
        fs::write(&artifact_path, "artifact")?;
        let scenario = FakeScenario {
            deployment_root,
            cleanup_log_path: cleanup_log_path.clone(),
            failure_artifact_path: artifact_path,
            verification: named_output_verification("ready"),
            extra_preparation_steps: vec![StepCommand::new("prepare-step", "/bin/sh").with_args([
                "-c".to_string(),
                format!("printf 'bootstrap' > '{}'", bootstrap_marker_path.display()),
            ])],
            should_fail_prepare: true,
        };

        let outcome = rehearse(temp_dir.path(), &backend, &scenario, &StepRunner::new());

        match outcome {
            RehearsalOutcome::Failed(failure) => {
                assert_eq!(failure.stage(), RehearsalStage::Prepare);
                let cleanup_report = failure
                    .cleanup_report()
                    .expect("failed rehearsals should include cleanup");
                assert_eq!(cleanup_report.results().len(), 1);
                assert_eq!(
                    cleanup_report.results()[0].action_name(),
                    "scenario-cleanup"
                );
                assert_eq!(fs::read_to_string(cleanup_log_path)?, "scenario-cleanup\n");
                assert!(bootstrap_marker_path.is_file());
            }
            RehearsalOutcome::Succeeded(_) => {
                panic!("expected prepare failure when bootstrap step is followed by a failure")
            }
        }

        Ok(())
    }

    #[test]
    fn deploy_failures_run_reverse_cleanup_and_preserve_actionable_failure_context()
    -> io::Result<()> {
        let temp_dir = TestDir::new("core-tests", "deploy-failure-cleanup")?;
        let cleanup_log_path = temp_dir.path().join("cleanup.log");
        let deployment_root = temp_dir.path().join("deployment");
        let backend_failure_artifact = temp_dir.path().join("backend.tfstate");
        let scenario_failure_artifact = temp_dir.path().join("artifact.txt");
        fs::create_dir_all(&deployment_root)?;
        fs::write(&backend_failure_artifact, "backend-state")?;
        fs::write(&scenario_failure_artifact, "artifact")?;
        let backend = FakeBackend {
            deployment_root: deployment_root.clone(),
            cleanup_log_path: cleanup_log_path.clone(),
            deploy_should_fail: true,
            destroy_should_fail: true,
            failure_artifact_path: Some(backend_failure_artifact.clone()),
        };
        let scenario = FakeScenario {
            deployment_root,
            cleanup_log_path: cleanup_log_path.clone(),
            failure_artifact_path: scenario_failure_artifact.clone(),
            verification: named_output_verification("ready"),
            extra_preparation_steps: Vec::new(),
            should_fail_prepare: false,
        };

        let outcome = rehearse(temp_dir.path(), &backend, &scenario, &StepRunner::new());

        match outcome {
            RehearsalOutcome::Failed(failure) => {
                assert_eq!(failure.stage(), RehearsalStage::Deploy);
                let cleanup_report = failure
                    .cleanup_report()
                    .expect("deploy failures should include cleanup");
                assert_eq!(
                    cleanup_report
                        .results()
                        .iter()
                        .map(|result| result.action_name())
                        .collect::<Vec<_>>(),
                    vec!["backend-destroy", "scenario-cleanup"]
                );
                assert_eq!(
                    fs::read_to_string(cleanup_log_path)?,
                    "backend-destroy\nscenario-cleanup\n"
                );
                assert!(cleanup_report.preserved_artifacts().iter().any(|artifact| {
                    artifact
                        .preserved_path
                        .ends_with("cleanup/backend-state.txt")
                        && artifact.source == backend_failure_artifact
                }));
                assert!(cleanup_report.preserved_artifacts().iter().any(|artifact| {
                    artifact
                        .preserved_path
                        .ends_with("cleanup/failure-artifact.txt")
                        && artifact.source == scenario_failure_artifact
                }));
                assert_eq!(cleanup_report.recovery_hints().len(), 1);
                assert_eq!(
                    cleanup_report.recovery_hints()[0].action_name,
                    "backend-destroy"
                );
                assert!(
                    cleanup_report.recovery_hints()[0]
                        .hint
                        .contains("Run the backend destroy command manually"),
                    "unexpected recovery hint: {}",
                    cleanup_report.recovery_hints()[0].hint
                );
            }
            RehearsalOutcome::Succeeded(_) => {
                panic!("expected deploy failure when the fake backend is configured to fail")
            }
        }

        Ok(())
    }

    fn named_output_verification(readiness_label: &str) -> ScenarioVerification {
        ScenarioVerification::new(
            readiness_label,
            ScenarioTarget::NamedOutput {
                key: "service".to_string(),
                value: "ok".to_string(),
            },
        )
    }

    fn successful_rehearsal_with(
        name: &str,
        verification: ScenarioVerification,
        extra_preparation_steps: Vec<StepCommand>,
    ) -> io::Result<SuccessfulRehearsalObservation> {
        let temp_dir = TestDir::new("core-tests", name)?;
        let cleanup_log_path = temp_dir.path().join("cleanup.log");
        let deployment_root = temp_dir.path().join("deployment");
        fs::create_dir_all(&deployment_root)?;
        let backend = FakeBackend {
            deployment_root: deployment_root.clone(),
            cleanup_log_path: cleanup_log_path.clone(),
            deploy_should_fail: false,
            destroy_should_fail: false,
            failure_artifact_path: None,
        };
        let artifact_path = temp_dir.path().join("artifact.txt");
        fs::write(&artifact_path, "artifact")?;
        let scenario = FakeScenario {
            deployment_root: deployment_root.clone(),
            cleanup_log_path,
            failure_artifact_path: artifact_path,
            verification,
            extra_preparation_steps,
            should_fail_prepare: false,
        };

        let outcome = rehearse(temp_dir.path(), &backend, &scenario, &StepRunner::new());
        let success = match outcome {
            RehearsalOutcome::Succeeded(success) => success,
            RehearsalOutcome::Failed(failure) => {
                panic!(
                    "expected success, got failure at {:?}: {}",
                    failure.stage(),
                    failure.error()
                )
            }
        };

        Ok(SuccessfulRehearsalObservation {
            cleanup_log: fs::read_to_string(temp_dir.path().join("cleanup.log"))?,
            cleanup_results: success
                .cleanup_report()
                .results()
                .iter()
                .map(|result| result.action_name().to_string())
                .collect(),
        })
    }

    struct SuccessfulRehearsalObservation {
        cleanup_log: String,
        cleanup_results: Vec<String>,
    }
}
