use crate::backends::terraform::{TerraformBackend, TerraformBackendConfig, TerraformBinary};
use crate::core::{RehearsalOutcome, rehearse};
use crate::scenarios::backend_rehearsal::{
    BackendRehearsalScenario, BackendRehearsalScenarioConfig,
};
use crate::steps::StepRunner;
use owo_colors::OwoColorize;
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process;

pub fn run() {
    match run_inner(env::args().collect()) {
        Ok(exit_code) => process::exit(exit_code),
        Err(message) => {
            dress_log(&message);
            process::exit(2);
        }
    }
}

fn run_inner(args: Vec<String>) -> Result<i32, String> {
    match args.get(1).map(String::as_str) {
        Some("smoke-backend") => run_smoke_backend(),
        Some("--help") | Some("-h") | None => {
            print_usage();
            Ok(0)
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{}", usage_text())),
    }
}

fn run_smoke_backend() -> Result<i32, String> {
    let config = load_smoke_config(&SmokeEnvironment::from_process())?;
    let backend = TerraformBackend::new(config.backend_config);
    let scenario = BackendRehearsalScenario::new(config.scenario_config);
    let runner = StepRunner::new();

    dress_log("starting backend rehearsal");
    dress_log(format!("runs root {}", config.runs_root.display()));
    dress_log(format!(
        "deployment root {}",
        config.deployment_root.display()
    ));

    match rehearse(&config.runs_root, &backend, &scenario, &runner) {
        RehearsalOutcome::Succeeded(success) => {
            dress_log(format!("run id {}", success.run_context().run_id()));
            dress_log(format!(
                "run directory {}",
                success.run_context().root_dir().display()
            ));
            dress_log("success");
            if let Some(summary_path) = success.summary_path() {
                dress_log(format!("summary {}", summary_path.display()));
            }
            if let Some(step_log_path) = success.step_log_path() {
                dress_log(format!("step log {}", step_log_path.display()));
            }
            Ok(0)
        }
        RehearsalOutcome::Failed(failure) => {
            dress_log(format!("run id {}", failure.run_context().run_id()));
            dress_log(format!(
                "run directory {}",
                failure.run_context().root_dir().display()
            ));
            dress_log(format!("failure during {}", failure.stage()));
            dress_log(format!("error {}", failure.error()));
            if let Some(summary_path) = failure.summary_path() {
                dress_log(format!("summary {}", summary_path.display()));
            }
            if let Some(step_log_path) = failure.step_log_path() {
                dress_log(format!("step log {}", step_log_path.display()));
            }
            dress_log(format!(
                "preserved artifacts {}",
                failure.run_context().preserved_dir().display()
            ));
            Ok(1)
        }
    }
}

struct SmokeConfig {
    runs_root: PathBuf,
    deployment_root: PathBuf,
    backend_config: TerraformBackendConfig,
    scenario_config: BackendRehearsalScenarioConfig,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SmokeEnvironment {
    values: BTreeMap<OsString, OsString>,
}

impl SmokeEnvironment {
    fn from_process() -> Self {
        Self {
            values: env::vars_os().collect(),
        }
    }

    #[cfg(test)]
    fn with_var(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    fn get(&self, key: &str) -> Option<&OsStr> {
        self.values.get(OsStr::new(key)).map(OsString::as_os_str)
    }
}

fn load_smoke_config(environment: &SmokeEnvironment) -> Result<SmokeConfig, String> {
    let deployment_root = required_env_path(environment, "DRESS_DEPLOYMENT_ROOT")?;
    let runs_root = optional_env_path(environment, "DRESS_RUNS_ROOT")
        .unwrap_or_else(|| deployment_root.join(".dress-runs"));
    let working_directory = optional_env_path(environment, "DRESS_WORKING_DIRECTORY");

    let mut backend_config =
        TerraformBackendConfig::default().with_binary(terraform_binary_from_env(environment)?);
    for path in env_path_list(environment, "DRESS_TF_VAR_FILES")? {
        backend_config = backend_config.with_var_file(path);
    }
    for path in env_path_list(environment, "DRESS_TF_BACKEND_CONFIG_FILES")? {
        backend_config = backend_config.with_backend_config_file(path);
    }

    let mut scenario_config = BackendRehearsalScenarioConfig::new(&deployment_root);
    if let Some(path) = working_directory.clone() {
        scenario_config = scenario_config.with_working_directory(path);
    }

    Ok(SmokeConfig {
        runs_root,
        deployment_root,
        backend_config,
        scenario_config,
    })
}

fn terraform_binary_from_env(environment: &SmokeEnvironment) -> Result<TerraformBinary, String> {
    match optional_env(environment, "DRESS_TERRAFORM_BINARY") {
        Some(value) if value == "terraform" => Ok(TerraformBinary::Terraform),
        Some(value) if value == "tofu" => Ok(TerraformBinary::OpenTofu),
        Some(value) if value.trim().is_empty() => {
            Err("`DRESS_TERRAFORM_BINARY` must not be empty".to_string())
        }
        Some(value) => Ok(TerraformBinary::Custom(PathBuf::from(value))),
        None => Ok(TerraformBinary::Terraform),
    }
}

fn env_path_list(environment: &SmokeEnvironment, key: &str) -> Result<Vec<PathBuf>, String> {
    match environment.get(key) {
        Some(value) => env::split_paths(value)
            .map(|path| {
                if path.as_os_str().is_empty() {
                    Err(format!("`{key}` contains an empty path entry"))
                } else {
                    Ok(path)
                }
            })
            .collect(),
        None => Ok(Vec::new()),
    }
}

fn optional_env(environment: &SmokeEnvironment, key: &str) -> Option<String> {
    environment
        .get(key)
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn required_env_path(environment: &SmokeEnvironment, key: &str) -> Result<PathBuf, String> {
    optional_env_path(environment, key)
        .ok_or_else(|| format!("missing required environment variable `{key}`"))
}

fn optional_env_path(environment: &SmokeEnvironment, key: &str) -> Option<PathBuf> {
    optional_env(environment, key).map(PathBuf::from)
}

fn print_usage() {
    dress_log(usage_text());
}

fn dress_log(message: impl AsRef<str>) {
    for line in message.as_ref().lines() {
        eprintln!("{} {}", dress_prefix(), line);
    }
}

fn dress_prefix() -> String {
    if io::stderr().is_terminal() {
        format!("{}", "[dress]".bright_cyan().dimmed())
    } else {
        "[dress]".to_string()
    }
}

fn usage_text() -> &'static str {
    "dress usage:
  cargo run -- smoke-backend

Required environment:
  DRESS_DEPLOYMENT_ROOT

Optional environment:
  DRESS_RUNS_ROOT
  DRESS_WORKING_DIRECTORY
  DRESS_TERRAFORM_BINARY
  DRESS_TF_VAR_FILES
  DRESS_TF_BACKEND_CONFIG_FILES

Deployment root contract:
  Terraform/OpenTofu must be runnable for apply and destroy"
}

#[cfg(test)]
mod tests {
    use super::{SmokeEnvironment, load_smoke_config, run_inner, terraform_binary_from_env};
    use crate::backends::terraform::TerraformBinary;
    use std::path::PathBuf;

    #[test]
    fn help_command_exits_successfully() {
        let exit_code = run_inner(vec!["dress".to_string(), "--help".to_string()])
            .expect("help should succeed");

        assert_eq!(exit_code, 0);
    }

    #[test]
    fn custom_terraform_binary_path_is_supported() {
        let environment =
            SmokeEnvironment::default().with_var("DRESS_TERRAFORM_BINARY", "/custom/tofu");

        let binary = terraform_binary_from_env(&environment).expect("custom binary should parse");

        assert_eq!(
            binary,
            TerraformBinary::Custom(PathBuf::from("/custom/tofu"))
        );
    }

    #[test]
    fn load_smoke_config_uses_explicit_environment_inputs() {
        let environment = SmokeEnvironment::default()
            .with_var("DRESS_DEPLOYMENT_ROOT", "/tmp/deploy")
            .with_var("DRESS_RUNS_ROOT", "/tmp/runs")
            .with_var("DRESS_WORKING_DIRECTORY", "/tmp/deploy/env/dev")
            .with_var("DRESS_TERRAFORM_BINARY", "tofu");

        let config =
            load_smoke_config(&environment).expect("config should load from explicit environment");

        assert_eq!(config.deployment_root, PathBuf::from("/tmp/deploy"));
        assert_eq!(config.runs_root, PathBuf::from("/tmp/runs"));
        assert_eq!(config.backend_config.binary(), &TerraformBinary::OpenTofu);
        assert_eq!(
            config.scenario_config.working_directory(),
            Some(PathBuf::from("/tmp/deploy/env/dev").as_path())
        );
    }
}
