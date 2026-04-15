use crate::backends::{
    BackendError, BackendOutputs, BackendRequest, BackendSession, DeploymentBackend,
};
use crate::cleanup::CleanupAction;
use crate::context::RunContext;
use crate::steps::{StepCommand, StepRunner};
use serde_json::Value;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs as unix_fs;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerraformExecutionMode {
    #[default]
    Isolated,
    NonIsolated,
}

impl TerraformExecutionMode {
    fn is_isolated(self) -> bool {
        matches!(self, Self::Isolated)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerraformBackendConfig {
    binary: TerraformBinary,
    execution_mode: TerraformExecutionMode,
    var_files: Vec<PathBuf>,
    backend_config_files: Vec<PathBuf>,
    auto_approve: bool,
}

impl Default for TerraformBackendConfig {
    fn default() -> Self {
        Self {
            binary: TerraformBinary::Terraform,
            execution_mode: TerraformExecutionMode::Isolated,
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

    pub fn execution_mode(&self) -> TerraformExecutionMode {
        self.execution_mode
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

    pub fn with_execution_mode(mut self, execution_mode: TerraformExecutionMode) -> Self {
        self.execution_mode = execution_mode;
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
        if self.config.execution_mode().is_isolated() {
            command = command.arg("-backend=false");
        } else {
            for path in self.config.backend_config_files() {
                command = command.arg(format!("-backend-config={}", path.display()));
            }
        }
        command
    }

    pub fn apply_command(&self, session: &BackendSession) -> StepCommand {
        let mut command = self.base_command("terraform-apply", session, "apply");
        if self.config.execution_mode().is_isolated() {
            command = command.arg(format!("-state={}", self.state_path(session).display()));
        }
        if self.config.auto_approve() {
            command = command.arg("-auto-approve");
        }
        for path in self.config.var_files() {
            command = command.arg(format!("-var-file={}", path.display()));
        }
        command
    }

    pub fn output_command(&self, session: &BackendSession) -> StepCommand {
        let mut command = self
            .base_command("terraform-output", session, "output")
            .arg("-json");
        if self.config.execution_mode().is_isolated() {
            command = command.arg(format!("-state={}", self.state_path(session).display()));
        }
        command
    }

    pub fn destroy_command(&self, session: &BackendSession) -> StepCommand {
        let mut command = self.base_command("terraform-destroy", session, "destroy");
        if self.config.execution_mode().is_isolated() {
            command = command.arg(format!("-state={}", self.state_path(session).display()));
        }
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
        let mut command = StepCommand::new(step_name, self.config.binary().program())
            .arg(subcommand)
            .with_current_dir(session.working_directory());
        for (key, value) in session.environment() {
            command = command.with_env(key.clone(), value.clone());
        }
        command
    }

    fn state_path(&self, session: &BackendSession) -> PathBuf {
        session.backend_work_dir().join("terraform.tfstate")
    }

    fn isolated_workspace_root(&self, run_context: &RunContext) -> PathBuf {
        run_context
            .work_dir()
            .join("backends")
            .join(self.name())
            .join("workspace")
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

    fn materialize_request(
        &self,
        run_context: &RunContext,
        request: &BackendRequest,
    ) -> Result<BackendRequest, BackendError> {
        if !self.config.execution_mode().is_isolated() {
            return Ok(request.clone());
        }

        let workspace_root = self.isolated_workspace_root(run_context);
        copy_deployment_tree(request.deployment_root(), &workspace_root)
            .map_err(|source| BackendError::io(self.name(), "initialize", source))?;

        let mut isolated_request = BackendRequest::new(&workspace_root);
        for (key, value) in request.environment() {
            isolated_request = isolated_request.with_env(key.clone(), value.clone());
        }
        isolated_request =
            isolated_request.with_env("TF_VAR_dress_run_id", run_context.run_id().to_string());

        let isolated_working_directory = match request.working_directory() {
            Some(working_directory) => {
                let relative_path = working_directory.strip_prefix(request.deployment_root()).map_err(|_| {
                    BackendError::invalid_configuration(
                        self.name(),
                        format!(
                            "isolated rehearsal requires the working directory to stay within the deployment root: {}",
                            working_directory.display()
                        ),
                    )
                })?;
                workspace_root.join(relative_path)
            }
            None => workspace_root.clone(),
        };

        Ok(isolated_request.with_working_directory(isolated_working_directory))
    }

    fn parse_outputs(&self, output_json: &str) -> Result<BackendOutputs, BackendError> {
        let value: Value = serde_json::from_str(output_json).map_err(|source| {
            BackendError::output_format(
                self.name(),
                "outputs",
                format!("expected terraform JSON object: {source}"),
            )
        })?;
        let object = value.as_object().ok_or_else(|| {
            BackendError::output_format(
                self.name(),
                "outputs",
                "top-level output was not an object",
            )
        })?;

        let mut outputs = BackendOutputs::new();
        for (key, entry) in object {
            let output_value = entry.get("value").ok_or_else(|| {
                BackendError::output_format(
                    self.name(),
                    "outputs",
                    format!("output `{key}` is missing a `value` field"),
                )
            })?;
            outputs.insert(key.clone(), render_output_value(output_value));
        }

        Ok(outputs)
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

        let materialized_request = self.materialize_request(run_context, request)?;
        let session = BackendSession::new(run_context, self.name(), &materialized_request);
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
        self.parse_outputs(&outcome.stdout_text())
    }

    fn destroy_action(&self, session: &BackendSession) -> CleanupAction {
        CleanupAction::new("terraform-destroy", self.destroy_command(session)).recovery_hint(
            "If destroy fails, inspect the terraform state and residual cloud resources before retrying cleanup.",
        )
    }
}

fn render_output_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn copy_deployment_tree(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let entry_type = entry.file_type()?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if file_name == ".terraform" || file_name == ".dress-runs" {
            continue;
        }

        if entry_type.is_symlink() {
            copy_symlink(&source_path, &destination_path)?;
        } else if entry_type.is_dir() {
            copy_deployment_tree(&source_path, &destination_path)?;
        } else if entry_type.is_file() {
            fs::copy(&source_path, &destination_path)?;
        } else {
            return Err(io::Error::other(format!(
                "unsupported deployment entry in isolated rehearsal workspace: {}",
                source_path.display()
            )));
        }
    }

    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> io::Result<()> {
    let link_target = fs::read_link(source)?;
    unix_fs::symlink(link_target, destination)
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::other(format!(
        "isolated rehearsal does not support symlinks on this platform: {}",
        source.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::{
        TerraformBackend, TerraformBackendConfig, TerraformBinary, TerraformExecutionMode,
    };
    use crate::backends::{BackendRequest, BackendSession, DeploymentBackend};
    use crate::context::{RunContext, RunId};
    use crate::steps::StepRunner;
    use std::env;
    use std::fs;
    use std::io;
    #[cfg(unix)]
    use std::os::unix::fs as unix_fs;
    use std::path::Path;
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

    fn platform_true_binary() -> &'static Path {
        if cfg!(target_os = "macos") {
            Path::new("/usr/bin/true")
        } else {
            Path::new("/bin/true")
        }
    }

    #[test]
    fn builds_apply_command_from_backend_config() {
        let backend = TerraformBackend::new(
            TerraformBackendConfig::default()
                .with_binary(TerraformBinary::OpenTofu)
                .with_execution_mode(TerraformExecutionMode::NonIsolated)
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
    fn applies_backend_request_environment_to_terraform_commands() {
        let backend = TerraformBackend::new(
            TerraformBackendConfig::default()
                .with_execution_mode(TerraformExecutionMode::NonIsolated),
        );
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-2000"));
        let request = BackendRequest::new("/tmp/scenario")
            .with_working_directory("/tmp/scenario")
            .with_env("BACKEND_WORKSPACE", "preview")
            .with_env("TF_IN_AUTOMATION", "1");
        let session = BackendSession::new(&run_context, "terraform", &request);

        let command = backend.apply_command(&session);

        assert_eq!(
            command.environment().get("BACKEND_WORKSPACE"),
            Some(&"preview".to_string())
        );
        assert_eq!(
            command.environment().get("TF_IN_AUTOMATION"),
            Some(&"1".to_string())
        );
    }

    #[test]
    fn builds_init_command_with_backend_config_files() {
        let backend = TerraformBackend::new(
            TerraformBackendConfig::default()
                .with_execution_mode(TerraformExecutionMode::NonIsolated)
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
    fn isolated_init_command_ignores_backend_config_files() {
        let backend = TerraformBackend::new(
            TerraformBackendConfig::default()
                .with_backend_config_file("backend/dev.hcl")
                .with_backend_config_file("backend/common.hcl"),
        );
        let session = backend_session("run-fixed-2007");

        let command = backend.init_command(&session);

        assert_eq!(
            command.args(),
            &["init".to_string(), "-backend=false".to_string()]
        );
    }

    #[test]
    fn initializes_backend_session_and_materializes_workspace() -> io::Result<()> {
        let temp_dir = TestDir::new("initialize")?;
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-terraform-init"));
        let scenario_root = temp_dir.path().join("scenario");
        let nested_working_directory = scenario_root.join("env/dev");
        fs::create_dir_all(&scenario_root)?;
        fs::create_dir_all(&nested_working_directory)?;
        fs::write(scenario_root.join("main.tf"), "terraform {}\n")?;
        fs::write(
            nested_working_directory.join("terraform.tfvars"),
            "name = \"dress\"\n",
        )?;
        fs::create_dir_all(scenario_root.join(".terraform"))?;
        fs::write(scenario_root.join(".terraform").join("ignore-me"), "cache")?;
        run_context.materialize()?;

        let backend = TerraformBackend::new(TerraformBackendConfig::default().with_binary(
            TerraformBinary::Custom(platform_true_binary().to_path_buf()),
        ));
        let request =
            BackendRequest::new(&scenario_root).with_working_directory(&nested_working_directory);
        let session = backend
            .initialize(&run_context, &request, &StepRunner::new())
            .map_err(io::Error::other)?;

        assert_eq!(session.backend_name(), "terraform");
        assert!(session.backend_work_dir().is_dir());
        assert!(session.backend_artifacts_dir().is_dir());
        assert_eq!(
            session.working_directory(),
            session.deployment_root().join("env/dev")
        );
        assert_eq!(
            fs::read_to_string(session.deployment_root().join("main.tf"))?,
            "terraform {}\n"
        );
        assert_eq!(
            fs::read_to_string(session.working_directory().join("terraform.tfvars"))?,
            "name = \"dress\"\n"
        );
        assert!(!session.deployment_root().join(".terraform").exists());
        assert_eq!(
            session.environment().get("TF_VAR_dress_run_id"),
            Some(&"run-fixed-terraform-init".to_string())
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn isolated_workspace_preserves_symlink_entries() -> io::Result<()> {
        let temp_dir = TestDir::new("symlink-copy")?;
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-terraform-symlink"));
        let scenario_root = temp_dir.path().join("scenario");
        let linked_target = scenario_root.join("shared.tfvars");
        fs::create_dir_all(&scenario_root)?;
        fs::write(&linked_target, "name = \"dress\"\n")?;
        unix_fs::symlink("shared.tfvars", scenario_root.join("terraform.tfvars"))?;
        run_context.materialize()?;

        let backend = TerraformBackend::new(TerraformBackendConfig::default().with_binary(
            TerraformBinary::Custom(platform_true_binary().to_path_buf()),
        ));
        let session = backend
            .initialize(
                &run_context,
                &BackendRequest::new(&scenario_root),
                &StepRunner::new(),
            )
            .map_err(io::Error::other)?;

        let copied_link = session.deployment_root().join("terraform.tfvars");
        let copied_target = fs::read_link(&copied_link)?;
        assert_eq!(copied_target, PathBuf::from("shared.tfvars"));
        assert_eq!(
            fs::read_to_string(session.deployment_root().join("shared.tfvars"))?,
            "name = \"dress\"\n"
        );

        Ok(())
    }

    #[test]
    fn isolated_commands_use_local_state_and_disable_remote_backend() {
        let backend = TerraformBackend::new(TerraformBackendConfig::default());
        let session = backend_session("run-fixed-2005");

        let init_command = backend.init_command(&session);
        let apply_command = backend.apply_command(&session);
        let output_command = backend.output_command(&session);
        let destroy_command = backend.destroy_command(&session);
        let state_arg = format!(
            "-state={}",
            session
                .backend_work_dir()
                .join("terraform.tfstate")
                .display()
        );

        assert_eq!(
            init_command.args(),
            &["init".to_string(), "-backend=false".to_string()]
        );
        assert!(apply_command.args().contains(&state_arg));
        assert!(output_command.args().contains(&state_arg));
        assert!(destroy_command.args().contains(&state_arg));
    }

    #[test]
    fn isolated_rehearsal_overrides_incoming_run_id_overlay() -> io::Result<()> {
        let temp_dir = TestDir::new("run-id-override")?;
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-terraform-override"));
        let scenario_root = temp_dir.path().join("scenario");
        fs::create_dir_all(&scenario_root)?;
        fs::write(scenario_root.join("main.tf"), "terraform {}\n")?;
        run_context.materialize()?;

        let backend = TerraformBackend::new(TerraformBackendConfig::default().with_binary(
            TerraformBinary::Custom(platform_true_binary().to_path_buf()),
        ));
        let request = BackendRequest::new(&scenario_root).with_env("TF_VAR_dress_run_id", "wrong");
        let session = backend
            .initialize(&run_context, &request, &StepRunner::new())
            .map_err(io::Error::other)?;

        assert_eq!(
            session.environment().get("TF_VAR_dress_run_id"),
            Some(&"run-fixed-terraform-override".to_string())
        );

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

    #[test]
    fn parses_terraform_output_json_into_normalized_values() {
        let backend = TerraformBackend::new(TerraformBackendConfig::default());
        let outputs = backend
            .parse_outputs(
                r#"{
                    "cluster_name": {"value": "dress-cluster"},
                    "desired_count": {"value": 2},
                    "service_tags": {"value": {"service":"dress"}}
                }"#,
            )
            .expect("terraform output should parse");

        assert_eq!(outputs.get("cluster_name"), Some("dress-cluster"));
        assert_eq!(outputs.get("desired_count"), Some("2"));
        assert_eq!(outputs.get("service_tags"), Some(r#"{"service":"dress"}"#));
    }

    #[test]
    fn destroy_action_reuses_destroy_command_shape() {
        let backend = TerraformBackend::new(
            TerraformBackendConfig::default()
                .with_binary(TerraformBinary::OpenTofu)
                .with_execution_mode(TerraformExecutionMode::NonIsolated)
                .with_var_file("env/dev.tfvars"),
        );
        let session = backend_session("run-fixed-2004");

        let action = backend.destroy_action(&session);

        assert_eq!(action.name(), "terraform-destroy");
        assert_eq!(
            action.command().args(),
            &[
                "destroy".to_string(),
                "-auto-approve".to_string(),
                "-var-file=env/dev.tfvars".to_string()
            ]
        );
    }

    #[test]
    fn isolated_rehearsal_rejects_working_directory_outside_deployment_root() -> io::Result<()> {
        let temp_dir = TestDir::new("outside-working-dir")?;
        let run_context = RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-2006"));
        let deployment_root = temp_dir.path().join("deployment");
        let outside_working_directory = temp_dir.path().join("outside");
        fs::create_dir_all(&deployment_root)?;
        fs::create_dir_all(&outside_working_directory)?;
        run_context.materialize()?;

        let backend = TerraformBackend::new(TerraformBackendConfig::default().with_binary(
            TerraformBinary::Custom(platform_true_binary().to_path_buf()),
        ));
        let request = BackendRequest::new(&deployment_root)
            .with_working_directory(&outside_working_directory);

        let error = backend
            .initialize(&run_context, &request, &StepRunner::new())
            .expect_err(
                "isolated rehearsal should reject working directories outside the deployment root",
            );

        assert!(
            error
                .to_string()
                .contains("working directory to stay within the deployment root"),
            "unexpected error: {error}"
        );
        Ok(())
    }

    fn backend_session(run_id: &str) -> BackendSession {
        let run_context = RunContext::with_run_id("/tmp/dress-runs", RunId::new(run_id));
        let request = BackendRequest::new("/tmp/scenario").with_working_directory("/tmp/scenario");
        BackendSession::new(&run_context, "terraform", &request)
    }
}
