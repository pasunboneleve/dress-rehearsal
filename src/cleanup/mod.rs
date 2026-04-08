//! CleanupManager and recovery-hint logic belong here.

use crate::context::RunContext;
use crate::steps::{StepCommand, StepError, StepOutcome, StepRunner};
use std::io;
use std::path::{Path, PathBuf};

/// Named cleanup unit registered against a run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CleanupAction {
    name: String,
    command: StepCommand,
    preserve_on_failure: Vec<CleanupArtifact>,
    recovery_hint: Option<String>,
}

impl CleanupAction {
    pub fn new(name: impl Into<String>, command: StepCommand) -> Self {
        Self {
            name: name.into(),
            command,
            preserve_on_failure: Vec::new(),
            recovery_hint: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn command(&self) -> &StepCommand {
        &self.command
    }

    pub fn preserve_on_failure(mut self, artifact: CleanupArtifact) -> Self {
        self.preserve_on_failure.push(artifact);
        self
    }

    pub fn recovery_hint(mut self, hint: impl Into<String>) -> Self {
        self.recovery_hint = Some(hint.into());
        self
    }

    pub fn preserved_artifacts(&self) -> &[CleanupArtifact] {
        &self.preserve_on_failure
    }

    pub fn recovery_hint_text(&self) -> Option<&str> {
        self.recovery_hint.as_deref()
    }
}

/// Artifact that should be copied into the preserved run area when failure cleanup runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CleanupArtifact {
    source: PathBuf,
    destination: PathBuf,
}

impl CleanupArtifact {
    pub fn new(source: impl Into<PathBuf>, destination: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
        }
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

#[derive(Clone, Debug, Default)]
pub struct CleanupManager {
    actions: Vec<CleanupAction>,
}

impl CleanupManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, action: CleanupAction) {
        self.actions.push(action);
    }

    pub fn registered_actions(&self) -> &[CleanupAction] {
        &self.actions
    }

    pub fn execute_teardown(&self, runner: &StepRunner) -> CleanupReport {
        let mut report = CleanupReport::default();

        for action in self.actions.iter().rev() {
            match runner.run_command(action.command()) {
                Ok(outcome) => report.results.push(CleanupResult::Succeeded {
                    action_name: action.name().to_string(),
                    outcome,
                }),
                Err(error) => report.results.push(CleanupResult::Failed {
                    action_name: action.name().to_string(),
                    error,
                }),
            }
        }

        report
    }

    pub fn execute_failure_cleanup(
        &self,
        runner: &StepRunner,
        run_context: &RunContext,
    ) -> CleanupReport {
        let mut report = CleanupReport::default();

        for action in self.actions.iter().rev() {
            if let Some(hint) = action.recovery_hint_text() {
                report.recovery_hints.push(hint.to_string());
            }

            for artifact in action.preserved_artifacts() {
                match run_context.preserve_file(artifact.source(), artifact.destination()) {
                    Ok(path) => report.preserved_artifacts.push(PreservedCleanupArtifact {
                        action_name: action.name().to_string(),
                        source: artifact.source().to_path_buf(),
                        preserved_path: path,
                    }),
                    Err(source) => report.preservation_errors.push(CleanupPreservationError {
                        action_name: action.name().to_string(),
                        source_path: artifact.source().to_path_buf(),
                        destination_path: artifact.destination().to_path_buf(),
                        source,
                    }),
                }
            }

            match runner.run_command(action.command()) {
                Ok(outcome) => report.results.push(CleanupResult::Succeeded {
                    action_name: action.name().to_string(),
                    outcome,
                }),
                Err(error) => report.results.push(CleanupResult::Failed {
                    action_name: action.name().to_string(),
                    error,
                }),
            }
        }

        report
    }
}

#[derive(Debug, Default)]
pub struct CleanupReport {
    results: Vec<CleanupResult>,
    preserved_artifacts: Vec<PreservedCleanupArtifact>,
    preservation_errors: Vec<CleanupPreservationError>,
    recovery_hints: Vec<String>,
}

impl CleanupReport {
    pub fn results(&self) -> &[CleanupResult] {
        &self.results
    }

    pub fn preserved_artifacts(&self) -> &[PreservedCleanupArtifact] {
        &self.preserved_artifacts
    }

    pub fn preservation_errors(&self) -> &[CleanupPreservationError] {
        &self.preservation_errors
    }

    pub fn recovery_hints(&self) -> &[String] {
        &self.recovery_hints
    }

    pub fn has_failures(&self) -> bool {
        self.results.iter().any(CleanupResult::is_failed) || !self.preservation_errors.is_empty()
    }
}

#[derive(Debug)]
pub enum CleanupResult {
    Succeeded {
        action_name: String,
        outcome: StepOutcome,
    },
    Failed {
        action_name: String,
        error: StepError,
    },
}

impl CleanupResult {
    pub fn action_name(&self) -> &str {
        match self {
            Self::Succeeded { action_name, .. } => action_name,
            Self::Failed { action_name, .. } => action_name,
        }
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreservedCleanupArtifact {
    pub action_name: String,
    pub source: PathBuf,
    pub preserved_path: PathBuf,
}

#[derive(Debug)]
pub struct CleanupPreservationError {
    pub action_name: String,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
    pub source: io::Error,
}

#[cfg(test)]
mod tests {
    use super::{CleanupAction, CleanupArtifact, CleanupManager, CleanupResult};
    use crate::context::{RunContext, RunId};
    use crate::steps::{StepCommand, StepError, StepRunner};
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
                "dress-rehearsal-cleanup-tests-{name}-{}",
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
    fn executes_cleanup_actions_in_reverse_registration_order() -> io::Result<()> {
        let temp_dir = TestDir::new("reverse-order")?;
        let output_file = temp_dir.path().join("cleanup.log");
        let mut manager = CleanupManager::new();
        let runner = StepRunner::new();

        manager.register(CleanupAction::new(
            "first",
            shell_command(format!(
                "printf 'first\\n' >> {}",
                shell_quote(&output_file)
            )),
        ));
        manager.register(CleanupAction::new(
            "second",
            shell_command(format!(
                "printf 'second\\n' >> {}",
                shell_quote(&output_file)
            )),
        ));

        let report = manager.execute_teardown(&runner);

        assert_eq!(report.results().len(), 2);
        assert_eq!(fs::read_to_string(output_file)?, "second\nfirst\n");
        Ok(())
    }

    #[test]
    fn failure_cleanup_preserves_artifacts_and_recovery_hints() -> io::Result<()> {
        let temp_dir = TestDir::new("failure-cleanup")?;
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-cleanup-fixed-0001"));
        run_context.materialize()?;

        let artifact_source = temp_dir.path().join("terraform.tfstate");
        fs::write(&artifact_source, "state")?;

        let mut manager = CleanupManager::new();
        let runner = StepRunner::new();
        manager.register(
            CleanupAction::new("destroy-stack", shell_command("printf 'cleanup'"))
                .preserve_on_failure(CleanupArtifact::new(
                    &artifact_source,
                    "cleanup/terraform.tfstate",
                ))
                .recovery_hint("Run `terraform destroy` manually if cleanup cannot complete."),
        );

        let report = manager.execute_failure_cleanup(&runner, &run_context);

        assert_eq!(report.results().len(), 1);
        assert_eq!(report.preserved_artifacts().len(), 1);
        assert_eq!(report.preservation_errors().len(), 0);
        assert_eq!(
            fs::read_to_string(&report.preserved_artifacts()[0].preserved_path)?,
            "state"
        );
        assert_eq!(
            report.recovery_hints(),
            &["Run `terraform destroy` manually if cleanup cannot complete."]
        );
        assert!(!report.has_failures());

        Ok(())
    }

    #[test]
    fn teardown_continues_collecting_failures_after_a_cleanup_error() {
        let mut manager = CleanupManager::new();
        let runner = StepRunner::new();

        manager.register(CleanupAction::new("first", shell_command("exit 23")));
        manager.register(CleanupAction::new("second", shell_command("printf 'ok'")));

        let report = manager.execute_teardown(&runner);

        assert_eq!(report.results().len(), 2);
        assert!(matches!(
            &report.results()[0],
            CleanupResult::Succeeded { action_name, .. } if action_name == "second"
        ));
        assert!(matches!(
            &report.results()[1],
            CleanupResult::Failed { action_name, error: StepError::Failed(_), .. }
                if action_name == "first"
        ));
        assert!(report.has_failures());
    }

    fn shell_command(script: impl Into<String>) -> StepCommand {
        StepCommand::new("cleanup-step", "/bin/sh").with_args(["-c".to_string(), script.into()])
    }

    fn shell_quote(path: &std::path::Path) -> String {
        let display = path.display().to_string();
        format!("'{}'", display.replace('\'', r#"'\''"#))
    }
}
