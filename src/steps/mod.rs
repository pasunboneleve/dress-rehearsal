//! StepRunner and step execution semantics belong here.

use owo_colors::OwoColorize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
        stdout_path: Option<PathBuf>,
        stderr_path: Option<PathBuf>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StepTerminalStatus {
    Succeeded,
    Failed { exit_code: Option<i32> },
    SpawnError { message: String },
}

impl StepTerminalStatus {
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::SpawnError { .. })
    }
}

pub trait StepEventSink {
    fn on_event(&mut self, event: StepEvent);
}

#[derive(Clone, Debug, Default)]
pub struct StepRunner {
    recorded_events: Arc<Mutex<Vec<StepEvent>>>,
    recorded_executions: Arc<Mutex<Vec<StepExecutionRecord>>>,
    artifact_root: Arc<Mutex<Option<PathBuf>>>,
    step_sequence: Arc<Mutex<u64>>,
}

impl StepRunner {
    pub fn new() -> Self {
        Self {
            recorded_events: Arc::new(Mutex::new(Vec::new())),
            recorded_executions: Arc::new(Mutex::new(Vec::new())),
            artifact_root: Arc::new(Mutex::new(None)),
            step_sequence: Arc::new(Mutex::new(0)),
        }
    }

    pub fn set_artifact_root(&self, artifact_root: impl Into<PathBuf>) {
        *self
            .artifact_root
            .lock()
            .expect("step artifact root mutex should not be poisoned") = Some(artifact_root.into());
    }

    pub fn recorded_events(&self) -> Vec<StepEvent> {
        self.recorded_events
            .lock()
            .expect("step event recording mutex should not be poisoned")
            .clone()
    }

    pub fn recorded_executions(&self) -> Vec<StepExecutionRecord> {
        self.recorded_executions
            .lock()
            .expect("step execution recording mutex should not be poisoned")
            .clone()
    }

    pub fn run_command(&self, command: &StepCommand) -> Result<StepOutcome, StepError> {
        let mut sink = NoopStepEventSink;
        self.run_command_with_sink(command, &mut sink)
    }

    pub fn run_command_with_sink(
        &self,
        command: &StepCommand,
        sink: &mut dyn StepEventSink,
    ) -> Result<StepOutcome, StepError> {
        let log_style = StepLogStyle::default();
        let started_at = Instant::now();
        log_style.print_step_started(command.name());

        let started_event = StepEvent::Started {
            step_name: command.name().clone(),
            command: command.display_command(),
        };
        self.record_event(&started_event);
        sink.on_event(started_event);

        let artifact_files = match self.open_artifact_files(command.name()) {
            Ok(files) => files,
            Err(source) => {
                let error = StepError::Spawn {
                    step_name: command.name().clone(),
                    command: command.display_command(),
                    source,
                };
                let finished_event = StepEvent::Finished {
                    step_name: command.name().clone(),
                    status: StepTerminalStatus::SpawnError {
                        message: error.to_string(),
                    },
                    stdout_path: None,
                    stderr_path: None,
                };
                self.record_execution(StepExecutionRecord {
                    step_name: command.name().clone(),
                    command: command.display_command(),
                    status: StepTerminalStatus::SpawnError {
                        message: error.to_string(),
                    },
                    stdout_path: None,
                    stderr_path: None,
                });
                self.record_event(&finished_event);
                sink.on_event(finished_event);
                log_style.print_step_finished(
                    command.name(),
                    &StepTerminalStatus::SpawnError {
                        message: error.to_string(),
                    },
                    started_at.elapsed(),
                );
                return Err(error);
            }
        };

        let mut process = match command
            .to_process_command()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(process) => process,
            Err(source) => {
                let stderr_path = artifact_files.stderr_path.clone();
                let stdout_path = artifact_files.stdout_path.clone();
                let error = StepError::Spawn {
                    step_name: command.name().clone(),
                    command: command.display_command(),
                    source,
                };
                if let Some(mut stderr_file) = artifact_files.stderr_file {
                    let _ = writeln!(stderr_file, "{error}");
                }
                let finished_event = StepEvent::Finished {
                    step_name: command.name().clone(),
                    status: StepTerminalStatus::SpawnError {
                        message: error.to_string(),
                    },
                    stdout_path: stdout_path.clone(),
                    stderr_path: stderr_path.clone(),
                };
                self.record_execution(StepExecutionRecord {
                    step_name: command.name().clone(),
                    command: command.display_command(),
                    status: StepTerminalStatus::SpawnError {
                        message: error.to_string(),
                    },
                    stdout_path,
                    stderr_path,
                });
                self.record_event(&finished_event);
                sink.on_event(finished_event);
                log_style.print_step_finished(
                    command.name(),
                    &StepTerminalStatus::SpawnError {
                        message: error.to_string(),
                    },
                    started_at.elapsed(),
                );
                return Err(error);
            }
        };

        let stdout = process
            .stdout
            .take()
            .expect("child stdout should be available when piped");
        let stderr = process
            .stderr
            .take()
            .expect("child stderr should be available when piped");
        let stdout_handle =
            spawn_stream_tee(stdout, artifact_files.stdout_file, StreamTarget::Stdout);
        let stderr_handle =
            spawn_stream_tee(stderr, artifact_files.stderr_file, StreamTarget::Stderr);
        let exit_status = process.wait().map_err(|source| StepError::Spawn {
            step_name: command.name().clone(),
            command: command.display_command(),
            source,
        })?;
        let stdout = stdout_handle
            .join()
            .expect("stdout tee thread should not panic");
        let stderr = stderr_handle
            .join()
            .expect("stderr tee thread should not panic");
        let stdout = stdout.map_err(|source| StepError::Spawn {
            step_name: command.name().clone(),
            command: command.display_command(),
            source,
        })?;
        let stderr = stderr.map_err(|source| StepError::Spawn {
            step_name: command.name().clone(),
            command: command.display_command(),
            source,
        })?;
        let outcome = StepOutcome::from_output(command, exit_status, stdout, stderr);

        if outcome.exit_status.success() {
            let finished_event = StepEvent::Finished {
                step_name: outcome.step_name.clone(),
                status: StepTerminalStatus::Succeeded,
                stdout_path: artifact_files.stdout_path.clone(),
                stderr_path: artifact_files.stderr_path.clone(),
            };
            self.record_execution(StepExecutionRecord {
                step_name: outcome.step_name.clone(),
                command: outcome.command.clone(),
                status: StepTerminalStatus::Succeeded,
                stdout_path: artifact_files.stdout_path.clone(),
                stderr_path: artifact_files.stderr_path.clone(),
            });
            self.record_event(&finished_event);
            sink.on_event(finished_event);
            log_style.print_step_finished(
                outcome.step_name(),
                &StepTerminalStatus::Succeeded,
                started_at.elapsed(),
            );
            Ok(outcome)
        } else {
            let finished_event = StepEvent::Finished {
                step_name: outcome.step_name.clone(),
                status: StepTerminalStatus::Failed {
                    exit_code: outcome.exit_code(),
                },
                stdout_path: artifact_files.stdout_path.clone(),
                stderr_path: artifact_files.stderr_path.clone(),
            };
            self.record_execution(StepExecutionRecord {
                step_name: outcome.step_name.clone(),
                command: outcome.command.clone(),
                status: StepTerminalStatus::Failed {
                    exit_code: outcome.exit_code(),
                },
                stdout_path: artifact_files.stdout_path.clone(),
                stderr_path: artifact_files.stderr_path.clone(),
            });
            self.record_event(&finished_event);
            sink.on_event(finished_event);
            log_style.print_step_finished(
                outcome.step_name(),
                &StepTerminalStatus::Failed {
                    exit_code: outcome.exit_code(),
                },
                started_at.elapsed(),
            );
            Err(StepError::Failed(outcome))
        }
    }

    fn record_event(&self, event: &StepEvent) {
        self.recorded_events
            .lock()
            .expect("step event recording mutex should not be poisoned")
            .push(event.clone());
    }

    fn record_execution(&self, execution: StepExecutionRecord) {
        self.recorded_executions
            .lock()
            .expect("step execution recording mutex should not be poisoned")
            .push(execution);
    }

    fn open_artifact_files(&self, step_name: &StepName) -> io::Result<StepArtifactFiles> {
        let artifact_root = self
            .artifact_root
            .lock()
            .expect("step artifact root mutex should not be poisoned")
            .clone();
        let Some(artifact_root) = artifact_root else {
            return Ok(StepArtifactFiles::default());
        };

        fs::create_dir_all(&artifact_root)?;
        let mut sequence = self
            .step_sequence
            .lock()
            .expect("step sequence mutex should not be poisoned");
        let step_index = *sequence;
        *sequence += 1;
        drop(sequence);

        let slug = slugify_step_name(step_name.as_str());
        let base = format!("{step_index:04}-{slug}");
        let stdout_path = artifact_root.join(format!("{base}.stdout.log"));
        let stderr_path = artifact_root.join(format!("{base}.stderr.log"));

        Ok(StepArtifactFiles {
            stdout_file: Some(File::create(&stdout_path)?),
            stderr_file: Some(File::create(&stderr_path)?),
            stdout_path: Some(stdout_path),
            stderr_path: Some(stderr_path),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepExecutionRecord {
    step_name: StepName,
    command: String,
    status: StepTerminalStatus,
    stdout_path: Option<PathBuf>,
    stderr_path: Option<PathBuf>,
}

impl StepExecutionRecord {
    pub fn step_name(&self) -> &StepName {
        &self.step_name
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn status(&self) -> &StepTerminalStatus {
        &self.status
    }

    pub fn stdout_path(&self) -> Option<&Path> {
        self.stdout_path.as_deref()
    }

    pub fn stderr_path(&self) -> Option<&Path> {
        self.stderr_path.as_deref()
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

struct NoopStepEventSink;

impl StepEventSink for NoopStepEventSink {
    fn on_event(&mut self, _event: StepEvent) {}
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

#[derive(Default)]
struct StepArtifactFiles {
    stdout_file: Option<File>,
    stderr_file: Option<File>,
    stdout_path: Option<PathBuf>,
    stderr_path: Option<PathBuf>,
}

enum StreamTarget {
    Stdout,
    Stderr,
}

fn spawn_stream_tee<R: Read + Send + 'static>(
    mut reader: R,
    mut artifact_file: Option<File>,
    target: StreamTarget,
) -> thread::JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buffer = [0_u8; 8192];

        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                if let Some(file) = artifact_file.as_mut() {
                    file.flush()?;
                }
                return Ok(captured);
            }

            let chunk = &buffer[..read];
            captured.extend_from_slice(chunk);

            if let Some(file) = artifact_file.as_mut() {
                file.write_all(chunk)?;
                file.flush()?;
            }

            match target {
                StreamTarget::Stdout => {
                    let mut terminal = io::stdout().lock();
                    terminal.write_all(chunk)?;
                    terminal.flush()?;
                }
                StreamTarget::Stderr => {
                    let mut terminal = io::stderr().lock();
                    terminal.write_all(chunk)?;
                    terminal.flush()?;
                }
            }
        }
    })
}

fn slugify_step_name(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_dash = false;

    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_was_dash = false;
        } else if !previous_was_dash {
            slug.push('-');
            previous_was_dash = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "step".to_string()
    } else {
        trimmed.to_string()
    }
}

struct StepLogStyle {
    enable_color: bool,
}

impl Default for StepLogStyle {
    fn default() -> Self {
        Self {
            enable_color: io::stderr().is_terminal(),
        }
    }
}

impl StepLogStyle {
    fn print_step_started(&self, step_name: &StepName) {
        eprintln!();
        eprintln!(
            "{} {} {}",
            self.render_dress_prefix(),
            self.render_step_prefix(None),
            step_name.as_str()
        );
    }

    fn print_step_finished(
        &self,
        step_name: &StepName,
        status: &StepTerminalStatus,
        elapsed: Duration,
    ) {
        eprintln!(
            "{} {} {} {}",
            self.render_dress_prefix(),
            self.render_step_prefix(Some(status)),
            step_name.as_str(),
            self.render_duration(elapsed)
        );
        if status.is_failed() {
            eprintln!(
                "{} {}",
                self.render_dress_prefix(),
                self.render_step_failed_line(step_name)
            );
        }
    }

    fn render_dress_prefix(&self) -> String {
        if self.enable_color {
            format!("{}", "[dress]".bright_cyan().dimmed())
        } else {
            "[dress]".to_string()
        }
    }

    fn render_step_prefix(&self, status: Option<&StepTerminalStatus>) -> String {
        let label = "==> Step:";
        if !self.enable_color {
            return label.to_string();
        }

        match status {
            Some(StepTerminalStatus::Succeeded) => format!("{}", label.green().bold()),
            Some(status) if status.is_failed() => format!("{}", label.red().bold()),
            _ => label.to_string(),
        }
    }

    fn render_step_failed_line(&self, step_name: &StepName) -> String {
        let line = format!("==> Step failed: {}", step_name.as_str());
        if self.enable_color {
            format!("{}", line.red().bold())
        } else {
            line
        }
    }

    fn render_duration(&self, elapsed: Duration) -> String {
        let rendered = format!("(completed in {})", human_duration(elapsed));
        if self.enable_color {
            format!("{}", rendered.yellow())
        } else {
            rendered
        }
    }
}

fn human_duration(duration: Duration) -> String {
    if duration.as_secs() >= 60 {
        let total_seconds = duration.as_secs();
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{minutes}m{seconds:02}s")
    } else {
        let seconds = duration.as_secs_f64();
        if seconds >= 1.0 {
            format!("{seconds:.1}s")
        } else {
            format!("{seconds:.2}s")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::human_duration;
    use super::{StepCommand, StepError, StepEvent, StepEventSink, StepRunner, StepTerminalStatus};
    use std::env;
    use std::time::Duration;

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
                stdout_path: None,
                stderr_path: None,
            }
        );
    }

    #[test]
    fn applies_explicit_environment_and_working_directory() {
        let runner = StepRunner::new();
        let expected_dir = env::temp_dir();
        let command = shell_command("printf '%s:%s' \"$STEP_VALUE\" \"$PWD\"")
            .with_env("STEP_VALUE", "configured")
            .with_current_dir(&expected_dir);

        let outcome = runner.run_command(&command).expect("step should succeed");
        let stdout = outcome.stdout_text();
        let (_, reported_dir) = stdout
            .split_once(':')
            .expect("stdout should contain configured directory output");
        let reported_dir = std::fs::canonicalize(reported_dir).expect("reported directory");
        let expected_dir = std::fs::canonicalize(expected_dir).expect("expected directory");

        assert!(outcome.stdout_text().starts_with("configured:"));
        assert_eq!(reported_dir, expected_dir);
        assert_eq!(runner.recorded_events().len(), 2);
    }

    #[test]
    fn formats_short_human_durations() {
        assert_eq!(human_duration(Duration::from_millis(1200)), "1.2s");
        assert_eq!(human_duration(Duration::from_secs(12)), "12.0s");
    }

    #[test]
    fn formats_minute_scale_human_durations() {
        assert_eq!(human_duration(Duration::from_secs(194)), "3m14s");
    }

    fn shell_command(script: &str) -> StepCommand {
        StepCommand::new("shell-step", shell_program())
            .with_args(["-c".to_string(), script.to_string()])
    }

    fn shell_program() -> &'static str {
        "/bin/sh"
    }
}
