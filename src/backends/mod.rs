//! DeploymentBackend abstractions and implementations belong here.

pub mod terraform;

use crate::cleanup::CleanupAction;
use crate::context::RunContext;
use crate::steps::{StepError, StepRunner};
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Materialized inputs that a scenario hands to a deployment backend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendRequest {
    deployment_root: PathBuf,
    working_directory: Option<PathBuf>,
    environment: BTreeMap<String, String>,
}

impl BackendRequest {
    pub fn new(deployment_root: impl Into<PathBuf>) -> Self {
        Self {
            deployment_root: deployment_root.into(),
            working_directory: None,
            environment: BTreeMap::new(),
        }
    }

    pub fn deployment_root(&self) -> &Path {
        &self.deployment_root
    }

    pub fn working_directory(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }

    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    pub fn with_working_directory(mut self, working_directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(working_directory.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }
}

/// Stable per-run backend workspace derived from `RunContext`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendSession {
    backend_name: String,
    deployment_root: PathBuf,
    working_directory: PathBuf,
    backend_work_dir: PathBuf,
    backend_artifacts_dir: PathBuf,
}

impl BackendSession {
    pub fn new(
        run_context: &RunContext,
        backend_name: impl Into<String>,
        request: &BackendRequest,
    ) -> Self {
        let backend_name = backend_name.into();
        let working_directory = request
            .working_directory()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| request.deployment_root().to_path_buf());
        let backend_work_dir = run_context.work_dir().join("backends").join(&backend_name);
        let backend_artifacts_dir = run_context
            .artifacts_dir()
            .join("backends")
            .join(&backend_name);

        Self {
            backend_name,
            deployment_root: request.deployment_root().to_path_buf(),
            working_directory,
            backend_work_dir,
            backend_artifacts_dir,
        }
    }

    pub fn backend_name(&self) -> &str {
        &self.backend_name
    }

    pub fn deployment_root(&self) -> &Path {
        &self.deployment_root
    }

    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    pub fn backend_work_dir(&self) -> &Path {
        &self.backend_work_dir
    }

    pub fn backend_artifacts_dir(&self) -> &Path {
        &self.backend_artifacts_dir
    }

    pub fn materialize(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.backend_work_dir)?;
        std::fs::create_dir_all(&self.backend_artifacts_dir)?;
        Ok(())
    }
}

/// Normalized backend outputs surfaced to scenarios and verification code.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BackendOutputs {
    values: BTreeMap<String, String>,
}

impl BackendOutputs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) -> Option<String> {
        self.values.insert(key.into(), value.into())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.values
    }
}

/// Narrow deployment engine contract. Concrete backends remain responsible for
/// how they turn sessions into real commands and state.
pub trait DeploymentBackend {
    fn name(&self) -> &'static str;

    fn initialize(
        &self,
        run_context: &RunContext,
        request: &BackendRequest,
        runner: &StepRunner,
    ) -> Result<BackendSession, BackendError>;

    fn deploy(&self, session: &BackendSession, runner: &StepRunner) -> Result<(), BackendError>;

    fn outputs(
        &self,
        session: &BackendSession,
        runner: &StepRunner,
    ) -> Result<BackendOutputs, BackendError>;

    fn destroy_action(&self, session: &BackendSession) -> CleanupAction;
}

#[derive(Debug)]
pub enum BackendError {
    InvalidConfiguration {
        backend_name: String,
        message: String,
    },
    Io {
        backend_name: String,
        operation: &'static str,
        source: io::Error,
    },
    Step {
        backend_name: String,
        operation: &'static str,
        source: StepError,
    },
    OutputFormat {
        backend_name: String,
        operation: &'static str,
        message: String,
    },
}

impl BackendError {
    pub fn invalid_configuration(
        backend_name: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::InvalidConfiguration {
            backend_name: backend_name.into(),
            message: message.into(),
        }
    }

    pub fn io(backend_name: impl Into<String>, operation: &'static str, source: io::Error) -> Self {
        Self::Io {
            backend_name: backend_name.into(),
            operation,
            source,
        }
    }

    pub fn step(
        backend_name: impl Into<String>,
        operation: &'static str,
        source: StepError,
    ) -> Self {
        Self::Step {
            backend_name: backend_name.into(),
            operation,
            source,
        }
    }

    pub fn output_format(
        backend_name: impl Into<String>,
        operation: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::OutputFormat {
            backend_name: backend_name.into(),
            operation,
            message: message.into(),
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration {
                backend_name,
                message,
            } => write!(f, "backend `{backend_name}` is invalid: {message}"),
            Self::Io {
                backend_name,
                operation,
                source,
            } => write!(
                f,
                "backend `{backend_name}` failed during {operation}: {source}"
            ),
            Self::Step {
                backend_name,
                operation,
                source,
            } => write!(
                f,
                "backend `{backend_name}` step failed during {operation}: {source}"
            ),
            Self::OutputFormat {
                backend_name,
                operation,
                message,
            } => write!(
                f,
                "backend `{backend_name}` returned invalid output during {operation}: {message}"
            ),
        }
    }
}

impl std::error::Error for BackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidConfiguration { .. } => None,
            Self::Io { source, .. } => Some(source),
            Self::Step { source, .. } => Some(source),
            Self::OutputFormat { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BackendOutputs, BackendRequest, BackendSession};
    use crate::context::{RunContext, RunId};
    use std::path::PathBuf;

    #[test]
    fn derives_backend_session_paths_from_run_context() {
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-1001"));
        let request =
            BackendRequest::new("/tmp/scenario").with_working_directory("/tmp/scenario/env");

        let session = BackendSession::new(&run_context, "terraform", &request);

        assert_eq!(session.backend_name(), "terraform");
        assert_eq!(session.deployment_root(), PathBuf::from("/tmp/scenario"));
        assert_eq!(
            session.working_directory(),
            PathBuf::from("/tmp/scenario/env")
        );
        assert_eq!(
            session.backend_work_dir(),
            PathBuf::from("/tmp/dress-runs/run-fixed-1001/work/backends/terraform")
        );
        assert_eq!(
            session.backend_artifacts_dir(),
            PathBuf::from("/tmp/dress-runs/run-fixed-1001/artifacts/backends/terraform")
        );
    }

    #[test]
    fn defaults_working_directory_to_deployment_root() {
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-1002"));
        let request = BackendRequest::new("/tmp/scenario");

        let session = BackendSession::new(&run_context, "cloudformation", &request);

        assert_eq!(session.working_directory(), PathBuf::from("/tmp/scenario"));
    }

    #[test]
    fn stores_normalized_backend_outputs() {
        let mut outputs = BackendOutputs::new();

        assert_eq!(outputs.insert("service_url", "https://example.test"), None);
        assert_eq!(
            outputs.insert("service_url", "https://replacement.test"),
            Some("https://example.test".to_string())
        );
        assert_eq!(outputs.get("service_url"), Some("https://replacement.test"));
    }

    #[test]
    fn request_keeps_generic_environment_inputs() {
        let request = BackendRequest::new("/tmp/scenario")
            .with_working_directory("/tmp/scenario/work")
            .with_env("AWS_REGION", "ap-southeast-2")
            .with_env("STACK_NAME", "dress-preview");

        assert_eq!(request.deployment_root(), PathBuf::from("/tmp/scenario"));
        assert_eq!(
            request.working_directory(),
            Some(PathBuf::from("/tmp/scenario/work").as_path())
        );
        assert_eq!(
            request.environment().get("AWS_REGION"),
            Some(&"ap-southeast-2".to_string())
        );
        assert_eq!(
            request.environment().get("STACK_NAME"),
            Some(&"dress-preview".to_string())
        );
    }
}
