//! StepRunner and step execution semantics belong here.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::sync::{Arc, Mutex};

/// Stable name for one orchestration step.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct StepName(String);

impl StepName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StepName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Explicit process invocation for a named step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepCommand {
    name: StepName,
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    environment: BTreeMap<String, String>,
}

impl StepCommand {
    pub fn new(name: impl Into<StepName>, program: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args: Vec::new(),
            current_dir: None,
            environment: BTreeMap::new(),
        }
    }

    pub fn name(&self) -> &StepName {
        &self.name
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn current_dir(&self) -> Option<&Path> {
        self.current_dir.as_deref()
    }

    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn with_current_dir(mut self, current_dir: impl Into<PathBuf>) -> Self {
        self.current_dir = Some(current_dir.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }

    pub fn display_command(&self) -> String {
        let mut rendered = render_shell_token(&self.program.display().to_string());
        for arg in &self.args {
            rendered.push(' ');
            rendered.push_str(&render_shell_token(arg));
        }
        rendered
    }

    fn to_process_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);

        if let Some(current_dir) = &self.current_dir {
            command.current_dir(current_dir);
        }

        for (key, value) in &self.environment {
            command.env(key, value);
        }

        command
    }
}

impl From<&str> for StepName {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for StepName {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Human-readable execution events. The first implementation keeps this simple
/// so the CLI can log start/finish boundaries without introducing a framework.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StepEvent {
    Started {
        step_name: StepName,
        command: String,
    },
    Finished {
        step_name: StepName,
        status: StepTerminalStatus,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StepTerminalStatus {
    Succeeded,
    Failed { exit_code: Option<i32> },
    SpawnError { message: String },
}

pub trait StepEventSink {
    fn on_event(&mut self, event: StepEvent);
}

#[derive(Clone, Debug, Default)]
pub struct StepRunner {
    recorded_events: Arc<Mutex<Vec<StepEvent>>>,
}

impl StepRunner {
    pub fn new() -> Self {
        Self {
            recorded_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn recorded_events(&self) -> Vec<StepEvent> {
        self.recorded_events
            .lock()
            .expect("step event recording mutex should not be poisoned")
            .clone()
    }

    pub fn run_command(&self, command: &StepCommand) -> Result<StepOutcome, StepError> {
        let mut sink = RecordingStepEventSink::new(self.recorded_events.clone());
        self.run_command_with_sink(command, &mut sink)
    }

    pub fn run_command_with_sink(
        &self,
        command: &StepCommand,
        sink: &mut dyn StepEventSink,
    ) -> Result<StepOutcome, StepError> {
        sink.on_event(StepEvent::Started {
            step_name: command.name().clone(),
            command: command.display_command(),
        });

        let output = match command.to_process_command().output() {
            Ok(output) => output,
            Err(source) => {
                let error = StepError::Spawn {
                    step_name: command.name().clone(),
                    command: command.display_command(),
                    source,
                };
                sink.on_event(StepEvent::Finished {
                    step_name: command.name().clone(),
                    status: StepTerminalStatus::SpawnError {
                        message: error.to_string(),
                    },
                });
                return Err(error);
            }
        };

        let outcome =
            StepOutcome::from_output(command, output.status, output.stdout, output.stderr);

        if outcome.exit_status.success() {
            sink.on_event(StepEvent::Finished {
                step_name: outcome.step_name.clone(),
                status: StepTerminalStatus::Succeeded,
            });
            Ok(outcome)
        } else {
            sink.on_event(StepEvent::Finished {
                step_name: outcome.step_name.clone(),
                status: StepTerminalStatus::Failed {
                    exit_code: outcome.exit_code(),
                },
            });
            Err(StepError::Failed(outcome))
        }
    }
}

/// Consistent captured output for one step invocation.
#[derive(Clone, Debug)]
pub struct StepOutcome {
    step_name: StepName,
    command: String,
    exit_status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl StepOutcome {
    fn from_output(
        command: &StepCommand,
        exit_status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    ) -> Self {
        Self {
            step_name: command.name().clone(),
            command: command.display_command(),
            exit_status,
            stdout,
            stderr,
        }
    }

    pub fn step_name(&self) -> &StepName {
        &self.step_name
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn exit_status(&self) -> ExitStatus {
        self.exit_status
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_status.code()
    }

    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

#[derive(Debug)]
pub enum StepError {
    Spawn {
        step_name: StepName,
        command: String,
        source: io::Error,
    },
    Failed(StepOutcome),
}

impl StepError {
    pub fn step_name(&self) -> &StepName {
        match self {
            Self::Spawn { step_name, .. } => step_name,
            Self::Failed(outcome) => outcome.step_name(),
        }
    }
}

impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn {
                step_name,
                command,
                source,
            } => write!(
                f,
                "step `{step_name}` failed to start command `{command}`: {source}"
            ),
            Self::Failed(outcome) => write!(
                f,
                "step `{}` exited unsuccessfully: {}",
                outcome.step_name(),
                outcome.exit_status()
            ),
        }
    }
}

impl std::error::Error for StepError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            Self::Failed(_) => None,
        }
    }
}

struct RecordingStepEventSink {
    recorded_events: Arc<Mutex<Vec<StepEvent>>>,
}

impl RecordingStepEventSink {
    fn new(recorded_events: Arc<Mutex<Vec<StepEvent>>>) -> Self {
        Self { recorded_events }
    }
}

impl StepEventSink for RecordingStepEventSink {
    fn on_event(&mut self, event: StepEvent) {
        self.recorded_events
            .lock()
            .expect("step event recording mutex should not be poisoned")
            .push(event);
    }
}

fn render_shell_token(token: &str) -> String {
    if token.is_empty() {
        return "''".to_string();
    }

    if token
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "/._-".contains(character))
    {
        return token.to_string();
    }

    let escaped = token.replace('\'', r#"'\''"#);
    format!("'{escaped}'")
}

#[cfg(test)]
mod tests {
    use super::{StepCommand, StepError, StepEvent, StepEventSink, StepRunner, StepTerminalStatus};
    use std::env;

    #[derive(Default)]
    struct RecordingSink {
        events: Vec<StepEvent>,
    }

    impl StepEventSink for RecordingSink {
        fn on_event(&mut self, event: StepEvent) {
            self.events.push(event);
        }
    }

    #[test]
    fn captures_stdout_and_stderr_for_successful_steps() {
        let runner = StepRunner::new();
        let command = shell_command("printf 'hello'; printf 'warn' >&2");

        let outcome = runner.run_command(&command).expect("step should succeed");

        assert_eq!(outcome.step_name().as_str(), "shell-step");
        assert_eq!(outcome.exit_code(), Some(0));
        assert_eq!(outcome.stdout_text(), "hello");
        assert_eq!(outcome.stderr_text(), "warn");
    }

    #[test]
    fn returns_consistent_failure_outcome_for_nonzero_exit() {
        let runner = StepRunner::new();
        let command = shell_command("printf 'out'; printf 'err' >&2; exit 17");

        let error = runner
            .run_command(&command)
            .expect_err("step should fail uniformly");

        match error {
            StepError::Failed(outcome) => {
                assert_eq!(outcome.step_name().as_str(), "shell-step");
                assert_eq!(outcome.exit_code(), Some(17));
                assert_eq!(outcome.stdout_text(), "out");
                assert_eq!(outcome.stderr_text(), "err");
            }
            other => panic!("expected failed outcome, got {other:?}"),
        }
    }

    #[test]
    fn returns_spawn_error_when_command_cannot_start() {
        let runner = StepRunner::new();
        let command = StepCommand::new("missing-command", "/definitely/not/a/real/program");

        let error = runner
            .run_command(&command)
            .expect_err("missing program should fail to spawn");

        match error {
            StepError::Spawn {
                step_name, command, ..
            } => {
                assert_eq!(step_name.as_str(), "missing-command");
                assert!(command.contains("/definitely/not/a/real/program"));
            }
            other => panic!("expected spawn error, got {other:?}"),
        }
    }

    #[test]
    fn emits_started_and_finished_events() {
        let runner = StepRunner::new();
        let command = shell_command("exit 0");
        let mut sink = RecordingSink::default();

        let _ = runner
            .run_command_with_sink(&command, &mut sink)
            .expect("step should succeed");

        assert_eq!(sink.events.len(), 2);
        assert_eq!(
            sink.events[0],
            StepEvent::Started {
                step_name: "shell-step".into(),
                command: format!("{} -c 'exit 0'", shell_program()),
            }
        );
        assert_eq!(
            sink.events[1],
            StepEvent::Finished {
                step_name: "shell-step".into(),
                status: StepTerminalStatus::Succeeded,
            }
        );
    }

    #[test]
    fn applies_explicit_environment_and_working_directory() {
        let runner = StepRunner::new();
        let command = shell_command("printf '%s:%s' \"$STEP_VALUE\" \"$PWD\"")
            .with_env("STEP_VALUE", "configured")
            .with_current_dir(env::temp_dir());

        let outcome = runner.run_command(&command).expect("step should succeed");

        assert!(outcome.stdout_text().starts_with("configured:"));
        assert!(
            outcome
                .stdout_text()
                .contains(&env::temp_dir().display().to_string())
        );
        assert_eq!(runner.recorded_events().len(), 2);
    }

    fn shell_command(script: &str) -> StepCommand {
        StepCommand::new("shell-step", shell_program())
            .with_args(["-c".to_string(), script.to_string()])
    }

    fn shell_program() -> &'static str {
        "/bin/sh"
    }
}
