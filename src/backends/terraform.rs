use crate::backends::{
    BackendError, BackendOutputs, BackendRequest, BackendSession, DeploymentBackend,
};
use crate::context::RunContext;
use crate::steps::{StepCommand, StepRunner};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerraformBinary {
    Terraform,
    OpenTofu,
    Custom(PathBuf),
}

impl TerraformBinary {
    pub fn program(&self) -> &Path {
        match self {
            Self::Terraform => Path::new("terraform"),
            Self::OpenTofu => Path::new("tofu"),
            Self::Custom(path) => path.as_path(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerraformBackendConfig {
    binary: TerraformBinary,
    var_files: Vec<PathBuf>,
    backend_config_files: Vec<PathBuf>,
    auto_approve: bool,
}

impl Default for TerraformBackendConfig {
    fn default() -> Self {
        Self {
            binary: TerraformBinary::Terraform,
            var_files: Vec::new(),
            backend_config_files: Vec::new(),
            auto_approve: true,
        }
    }
}

impl TerraformBackendConfig {
    pub fn binary(&self) -> &TerraformBinary {
        &self.binary
    }

    pub fn var_files(&self) -> &[PathBuf] {
        &self.var_files
    }

    pub fn backend_config_files(&self) -> &[PathBuf] {
        &self.backend_config_files
    }

    pub fn auto_approve(&self) -> bool {
        self.auto_approve
    }

    pub fn with_binary(mut self, binary: TerraformBinary) -> Self {
        self.binary = binary;
        self
    }

    pub fn with_var_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.var_files.push(path.into());
        self
    }

    pub fn with_backend_config_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.backend_config_files.push(path.into());
        self
    }

    pub fn with_auto_approve(mut self, auto_approve: bool) -> Self {
        self.auto_approve = auto_approve;
        self
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerraformBackend {
    config: TerraformBackendConfig,
}

impl TerraformBackend {
    pub fn new(config: TerraformBackendConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &TerraformBackendConfig {
        &self.config
    }

    pub fn init_command(&self, session: &BackendSession) -> StepCommand {
        let mut command = self.base_command("terraform-init", session, "init");
        for path in self.config.backend_config_files() {
            command = command.arg(format!("-backend-config={}", path.display()));
        }
        command
    }

    pub fn apply_command(&self, session: &BackendSession) -> StepCommand {
        let mut command = self.base_command("terraform-apply", session, "apply");
        if self.config.auto_approve() {
            command = command.arg("-auto-approve");
        }
        for path in self.config.var_files() {
            command = command.arg(format!("-var-file={}", path.display()));
        }
        command
    }

    pub fn output_command(&self, session: &BackendSession) -> StepCommand {
        self.base_command("terraform-output", session, "output")
            .arg("-json")
    }

    pub fn destroy_command(&self, session: &BackendSession) -> StepCommand {
        let mut command = self.base_command("terraform-destroy", session, "destroy");
        if self.config.auto_approve() {
            command = command.arg("-auto-approve");
        }
        for path in self.config.var_files() {
            command = command.arg(format!("-var-file={}", path.display()));
        }
        command
    }

    fn base_command(
        &self,
        step_name: &'static str,
        session: &BackendSession,
        subcommand: &'static str,
    ) -> StepCommand {
        StepCommand::new(step_name, self.config.binary().program())
            .arg(subcommand)
            .with_current_dir(session.working_directory())
    }

    fn validate_request(&self, request: &BackendRequest) -> Result<(), BackendError> {
        if !request.deployment_root().exists() {
            return Err(BackendError::invalid_configuration(
                self.name(),
                format!(
                    "deployment root does not exist: {}",
                    request.deployment_root().display()
                ),
            ));
        }

        Ok(())
    }
}

impl DeploymentBackend for TerraformBackend {
    fn name(&self) -> &'static str {
        "terraform"
    }

    fn initialize(
        &self,
        run_context: &RunContext,
        request: &BackendRequest,
        runner: &StepRunner,
    ) -> Result<BackendSession, BackendError> {
        self.validate_request(request)?;

        let session = BackendSession::new(run_context, self.name(), request);
        session
            .materialize()
            .map_err(|source| BackendError::io(self.name(), "initialize", source))?;

        let init_command = self.init_command(&session);
        runner
            .run_command(&init_command)
            .map_err(|source| BackendError::step(self.name(), "initialize", source))?;

        Ok(session)
    }

    fn deploy(&self, session: &BackendSession, runner: &StepRunner) -> Result<(), BackendError> {
        let apply_command = self.apply_command(session);
        runner
            .run_command(&apply_command)
            .map_err(|source| BackendError::step(self.name(), "deploy", source))?;
        Ok(())
    }

    fn outputs(
        &self,
        session: &BackendSession,
        runner: &StepRunner,
    ) -> Result<BackendOutputs, BackendError> {
        let output_command = self.output_command(session);
        let outcome = runner
            .run_command(&output_command)
            .map_err(|source| BackendError::step(self.name(), "outputs", source))?;

        let mut outputs = BackendOutputs::new();
        outputs.insert("raw_output_json", outcome.stdout_text());
        Ok(outputs)
    }

    fn destroy(&self, session: &BackendSession, runner: &StepRunner) -> Result<(), BackendError> {
        let destroy_command = self.destroy_command(session);
        runner
            .run_command(&destroy_command)
            .map_err(|source| BackendError::step(self.name(), "destroy", source))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{TerraformBackend, TerraformBackendConfig, TerraformBinary};
    use crate::backends::{BackendRequest, BackendSession, DeploymentBackend};
    use crate::context::{RunContext, RunId};
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
                "dress-rehearsal-terraform-tests-{name}-{}",
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
    fn builds_apply_command_from_backend_config() {
        let backend = TerraformBackend::new(
            TerraformBackendConfig::default()
                .with_binary(TerraformBinary::OpenTofu)
                .with_var_file("env/dev.tfvars")
                .with_auto_approve(true),
        );
        let session = backend_session("run-fixed-2001");

        let command = backend.apply_command(&session);

        assert_eq!(command.name().as_str(), "terraform-apply");
        assert_eq!(command.program(), PathBuf::from("tofu"));
        assert_eq!(
            command.args(),
            &[
                "apply".to_string(),
                "-auto-approve".to_string(),
                "-var-file=env/dev.tfvars".to_string()
            ]
        );
    }

    #[test]
    fn builds_init_command_with_backend_config_files() {
        let backend = TerraformBackend::new(
            TerraformBackendConfig::default()
                .with_backend_config_file("backend/dev.hcl")
                .with_backend_config_file("backend/common.hcl"),
        );
        let session = backend_session("run-fixed-2002");

        let command = backend.init_command(&session);

        assert_eq!(
            command.args(),
            &[
                "init".to_string(),
                "-backend-config=backend/dev.hcl".to_string(),
                "-backend-config=backend/common.hcl".to_string()
            ]
        );
    }

    #[test]
    fn initializes_backend_session_and_materializes_workspace() -> io::Result<()> {
        let temp_dir = TestDir::new("initialize")?;
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-terraform-init"));
        let scenario_root = temp_dir.path().join("scenario");
        fs::create_dir_all(&scenario_root)?;
        run_context.materialize()?;

        let backend = TerraformBackend::new(
            TerraformBackendConfig::default()
                .with_binary(TerraformBinary::Custom(PathBuf::from("/bin/true"))),
        );
        let request = BackendRequest::new(&scenario_root);
        let session = backend
            .initialize(&run_context, &request, &StepRunner::new())
            .map_err(io::Error::other)?;

        assert_eq!(session.backend_name(), "terraform");
        assert!(session.backend_work_dir().is_dir());
        assert!(session.backend_artifacts_dir().is_dir());
        Ok(())
    }

    #[test]
    fn rejects_missing_deployment_root() {
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-2003"));
        let backend = TerraformBackend::new(TerraformBackendConfig::default());
        let request = BackendRequest::new("/definitely/not/a/real/dir");
        let error = backend
            .initialize(&run_context, &request, &StepRunner::new())
            .expect_err("missing deployment root should be invalid");

        assert!(
            error.to_string().contains("deployment root does not exist"),
            "unexpected error: {error}"
        );
    }

    fn backend_session(run_id: &str) -> BackendSession {
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new(run_id));
        let request = BackendRequest::new("/tmp/scenario").with_working_directory("/tmp/scenario");
        BackendSession::new(&run_context, "terraform", &request)
    }
}
