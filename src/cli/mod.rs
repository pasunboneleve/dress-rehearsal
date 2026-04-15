use crate::backends::terraform::{
    TerraformBackend, TerraformBackendConfig, TerraformBinary, TerraformExecutionMode,
};
use crate::core::{RehearsalFailure, RehearsalOutcome, rehearse};
use crate::scenarios::backend_rehearsal::{
    BackendRehearsalScenario, BackendRehearsalScenarioConfig,
};
use crate::steps::StepRunner;
use owo_colors::OwoColorize;
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::io::{self, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

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
    match select_command(&args)? {
        CommandSelection::RunBackendRehearsal(options) => run_backend_rehearsal(options),
        CommandSelection::PrintHelp => {
            print_help();
            Ok(0)
        }
        CommandSelection::PrintVersion => {
            print_version();
            Ok(0)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CommandSelection {
    RunBackendRehearsal(RehearsalOptions),
    PrintHelp,
    PrintVersion,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct RehearsalOptions {
    disable_isolation: bool,
    yes: bool,
}

fn select_command(args: &[String]) -> Result<CommandSelection, String> {
    let mut options = RehearsalOptions::default();
    let mut saw_unknown = None;

    for arg in args.iter().skip(1) {
        match arg.as_str() {
            "--help" | "-h" => return Ok(CommandSelection::PrintHelp),
            "--version" | "-V" | "version" => return Ok(CommandSelection::PrintVersion),
            "--disable-isolation" => options.disable_isolation = true,
            "--yes" | "-y" => options.yes = true,
            other if saw_unknown.is_none() => saw_unknown = Some(other.to_string()),
            _ => {}
        }
    }

    if let Some(unknown) = saw_unknown {
        return Err(format!("unknown command `{unknown}`\n\n{}", usage_text()));
    }

    Ok(CommandSelection::RunBackendRehearsal(options))
}

fn run_backend_rehearsal(options: RehearsalOptions) -> Result<i32, String> {
    let execution_mode = if options.disable_isolation {
        confirm_disable_isolation(options.yes)?;
        TerraformExecutionMode::NonIsolated
    } else {
        TerraformExecutionMode::Isolated
    };

    let config = load_smoke_config(&SmokeEnvironment::from_process(), execution_mode)?;
    let backend = TerraformBackend::new(config.backend_config);
    let scenario = BackendRehearsalScenario::new(config.scenario_config);
    let runner = StepRunner::new();

    dress_log("starting backend rehearsal");
    dress_log(format!(
        "execution mode {}",
        if execution_mode.is_isolated() {
            "isolated"
        } else {
            "non-isolated (shared state)"
        }
    ));
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
            dress_log_failure_error(&failure);
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

fn load_smoke_config(
    environment: &SmokeEnvironment,
    execution_mode: TerraformExecutionMode,
) -> Result<SmokeConfig, String> {
    let deployment_root = deployment_root_from_env(environment)?;
    let runs_root = optional_env_path(environment, "DRESS_RUNS_ROOT")
        .unwrap_or_else(|| deployment_root.join(".dress-runs"));
    let working_directory = optional_env_path(environment, "DRESS_WORKING_DIRECTORY");

    let mut backend_config = TerraformBackendConfig::default()
        .with_binary(terraform_binary_from_env(environment)?)
        .with_execution_mode(execution_mode);
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

fn confirm_disable_isolation(skip_confirmation: bool) -> Result<(), String> {
    dress_log_warning(
        "WARNING: --disable-isolation runs against shared state with no isolation guarantees",
    );
    dress_log_warning("This can modify or destroy real infrastructure.");

    if skip_confirmation {
        dress_log("confirmation skipped via --yes");
        return Ok(());
    }

    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(
            "--disable-isolation requires interactive confirmation or --yes in non-interactive mode"
                .to_string(),
        );
    }

    eprint!(
        "{} Type 'disable-isolation' to confirm: ",
        dress_prefix_warning()
    );
    io::stderr().flush().ok();

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| format!("failed to read confirmation: {e}"))?;

    if input.trim() == "disable-isolation" {
        dress_log("isolation disabled by operator confirmation");
        Ok(())
    } else {
        Err("confirmation failed: did not type 'disable-isolation'".to_string())
    }
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

fn optional_env_path(environment: &SmokeEnvironment, key: &str) -> Option<PathBuf> {
    optional_env(environment, key).map(PathBuf::from)
}

fn deployment_root_from_env(environment: &SmokeEnvironment) -> Result<PathBuf, String> {
    match optional_env_path(environment, "DRESS_DEPLOYMENT_ROOT") {
        Some(path) => Ok(path),
        None => env::current_dir()
            .map_err(|source| format!("failed to determine current directory: {source}")),
    }
}

fn print_help() {
    print_lines(&help_text());
}

fn print_version() {
    print_lines(&version_text());
}

fn dress_log(message: impl AsRef<str>) {
    for line in message.as_ref().lines() {
        eprintln!("{} {}", dress_prefix(), line);
    }
}

fn dress_log_failure_error(failure: &RehearsalFailure) {
    let message = failure.error().to_string();
    if failure_summary_should_use_warning_style(&message, failure.failed_step_stderr_path()) {
        dress_log_warning_full_line(format!("error {message}"));
    } else {
        dress_log(format!("error {message}"));
    }
}

fn print_lines(message: &str) {
    for line in message.lines() {
        println!("{line}");
    }
}

fn dress_prefix() -> String {
    if io::stderr().is_terminal() {
        format!("{}", "[dress]".bright_cyan().dimmed())
    } else {
        "[dress]".to_string()
    }
}

fn dress_log_warning(message: impl AsRef<str>) {
    for line in message.as_ref().lines() {
        eprintln!("{} {}", dress_prefix_warning(), line);
    }
}

fn dress_log_warning_full_line(message: impl AsRef<str>) {
    let is_terminal = io::stderr().is_terminal();
    for line in message.as_ref().lines() {
        if is_terminal {
            eprintln!("{} {}", dress_prefix_warning(), line.yellow().bold());
        } else {
            eprintln!("{} {}", dress_prefix_warning(), line);
        }
    }
}

fn dress_prefix_warning() -> String {
    if io::stderr().is_terminal() {
        format!("{}", "[dress]".yellow().bold())
    } else {
        "[dress]".to_string()
    }
}

fn failure_summary_should_use_warning_style(
    message: &str,
    failed_step_stderr_path: Option<&std::path::Path>,
) -> bool {
    failure_error_looks_like_existing_resource_conflict(message)
        || failed_step_stderr_path_looks_like_existing_resource_conflict(failed_step_stderr_path)
}

fn failure_error_looks_like_existing_resource_conflict(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    normalized.contains("already exists") || normalized.contains("entityalreadyexists")
}

fn failed_step_stderr_path_looks_like_existing_resource_conflict(
    stderr_path: Option<&std::path::Path>,
) -> bool {
    let Some(stderr_path) = stderr_path else {
        return false;
    };

    match read_stderr_excerpt(stderr_path, 64 * 1024) {
        Ok(stderr) => failure_error_looks_like_existing_resource_conflict(&stderr),
        Err(_) => false,
    }
}

fn read_stderr_excerpt(path: &std::path::Path, max_bytes: u64) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let file_len = file.metadata()?.len();
    let start = file_len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;

    let mut buffer = Vec::new();
    file.take(max_bytes).read_to_end(&mut buffer)?;

    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

fn usage_text() -> &'static str {
    "dress usage:
  dress [--disable-isolation [--yes]]
  dress --help
  dress --version
  dress version"
}

fn version_text() -> String {
    format!("dress {}", VERSION)
}

fn help_text() -> String {
    let mut text = String::new();
    let _ = writeln!(text, "dress {}", VERSION);
    let _ = writeln!(text);
    let _ = writeln!(
        text,
        "Infrastructure rehearsal CLI for backend-tool apply/destroy runs."
    );
    let _ = writeln!(text);
    let _ = writeln!(text, "Current first-version scope:");
    let _ = writeln!(text, "- runs one backend rehearsal flow");
    let _ = writeln!(text, "- current backend implementation: Terraform/OpenTofu");
    let _ = writeln!(
        text,
        "- captures step logs, summaries, and preserved artifacts"
    );
    let _ = writeln!(
        text,
        "- does not model provider services or perform application health checks"
    );
    let _ = writeln!(text);
    let _ = writeln!(text, "What happens when you run `dress`:");
    let _ = writeln!(
        text,
        "- loads the backend rehearsal configuration from explicit environment variables and the current working directory"
    );
    let _ = writeln!(
        text,
        "- materializes an isolated run directory under `DRESS_RUNS_ROOT` or `<deployment-root>/.dress-runs`"
    );
    let _ = writeln!(
        text,
        "- runs backend init/apply, collects outputs, and then runs backend destroy"
    );
    let _ = writeln!(
        text,
        "- preserves failure evidence when apply, verification, or cleanup fails"
    );
    let _ = writeln!(text);
    let _ = writeln!(text, "Minimal requirements:");
    let _ = writeln!(
        text,
        "- run `dress` from a backend deployment directory, or set `DRESS_DEPLOYMENT_ROOT` explicitly"
    );
    let _ = writeln!(
        text,
        "- Terraform or OpenTofu is installed, unless `DRESS_TERRAFORM_BINARY` points elsewhere"
    );
    let _ = writeln!(
        text,
        "- the selected backend configuration can complete apply and destroy from that directory"
    );
    let _ = writeln!(text);
    let _ = writeln!(text, "Important environment variables:");
    let _ = writeln!(text, "- optional override: `DRESS_DEPLOYMENT_ROOT`");
    let _ = writeln!(
        text,
        "  when unset, `dress` uses the current working directory as the deployment root"
    );
    let _ = writeln!(text, "- optional: `DRESS_RUNS_ROOT`");
    let _ = writeln!(text, "- optional: `DRESS_WORKING_DIRECTORY`");
    let _ = writeln!(
        text,
        "- optional: `DRESS_TERRAFORM_BINARY` (`terraform`, `tofu`, or a custom path)"
    );
    let _ = writeln!(text, "- optional: `DRESS_TF_VAR_FILES` (path list)");
    let _ = writeln!(
        text,
        "- optional: `DRESS_TF_BACKEND_CONFIG_FILES` (path list)"
    );
    let _ = writeln!(text);
    let _ = writeln!(text, "Commands:");
    let _ = writeln!(text, "- `dress` runs the current backend rehearsal flow");
    let _ = writeln!(
        text,
        "- `dress version` and `dress --version` print the CLI version"
    );
    let _ = writeln!(text);
    let _ = writeln!(text, "Flags:");
    let _ = writeln!(
        text,
        "- `--disable-isolation` runs against shared state with no isolation guarantees"
    );
    let _ = writeln!(
        text,
        "  WARNING: this can modify or destroy real infrastructure"
    );
    let _ = writeln!(
        text,
        "  requires interactive confirmation or `--yes` to proceed"
    );
    let _ = writeln!(
        text,
        "- `--yes` or `-y` skips interactive confirmation for destructive operations"
    );
    text
}

#[cfg(test)]
mod tests {
    use super::{
        CommandSelection, RehearsalOptions, SmokeEnvironment,
        failed_step_stderr_path_looks_like_existing_resource_conflict,
        failure_error_looks_like_existing_resource_conflict,
        failure_summary_should_use_warning_style, help_text, load_smoke_config, run_inner,
        select_command, terraform_binary_from_env, version_text,
    };
    use crate::backends::terraform::{TerraformBinary, TerraformExecutionMode};
    use crate::test_support::TestDir;
    use std::env;
    use std::path::PathBuf;

    #[test]
    fn help_command_exits_successfully() {
        let exit_code = run_inner(vec!["dress".to_string(), "--help".to_string()])
            .expect("help should succeed");

        assert_eq!(exit_code, 0);
    }

    #[test]
    fn version_command_exits_successfully() {
        let exit_code = run_inner(vec!["dress".to_string(), "version".to_string()])
            .expect("version should succeed");

        assert_eq!(exit_code, 0);
    }

    #[test]
    fn version_flag_exits_successfully() {
        let exit_code = run_inner(vec!["dress".to_string(), "--version".to_string()])
            .expect("version flag should succeed");

        assert_eq!(exit_code, 0);
    }

    #[test]
    fn default_invocation_runs_backend_flow() {
        let selection =
            select_command(&["dress".to_string()]).expect("default invocation should dispatch");

        assert!(matches!(
            selection,
            CommandSelection::RunBackendRehearsal(RehearsalOptions {
                disable_isolation: false,
                yes: false
            })
        ));
    }

    #[test]
    fn removed_smoke_backend_subcommand_is_rejected() {
        let error = run_inner(vec!["dress".to_string(), "smoke-backend".to_string()])
            .expect_err("legacy subcommand should be rejected");

        assert!(error.contains("unknown command `smoke-backend`"));
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

        let config = load_smoke_config(&environment, TerraformExecutionMode::Isolated)
            .expect("config should load from explicit environment");

        assert_eq!(config.deployment_root, PathBuf::from("/tmp/deploy"));
        assert_eq!(config.runs_root, PathBuf::from("/tmp/runs"));
        assert_eq!(config.backend_config.binary(), &TerraformBinary::OpenTofu);
        assert_eq!(
            config.scenario_config.working_directory(),
            Some(PathBuf::from("/tmp/deploy/env/dev").as_path())
        );
    }

    #[test]
    fn load_smoke_config_falls_back_to_current_directory() {
        let environment = SmokeEnvironment::default();
        let current_dir = env::current_dir().expect("current directory should resolve");

        let config = load_smoke_config(&environment, TerraformExecutionMode::Isolated)
            .expect("config should use current directory fallback");

        assert_eq!(config.deployment_root, current_dir);
        assert_eq!(config.runs_root, current_dir.join(".dress-runs"));
    }

    #[test]
    fn explicit_deployment_root_overrides_current_directory() {
        let environment =
            SmokeEnvironment::default().with_var("DRESS_DEPLOYMENT_ROOT", "/tmp/deploy");

        let config = load_smoke_config(&environment, TerraformExecutionMode::Isolated)
            .expect("explicit deployment root should win");

        assert_eq!(config.deployment_root, PathBuf::from("/tmp/deploy"));
    }

    #[test]
    fn help_text_describes_current_scope_honestly() {
        let help = help_text();

        assert!(help.contains("Infrastructure rehearsal CLI"));
        assert!(help.contains("current backend implementation: Terraform/OpenTofu"));
        assert!(help.contains("does not model provider services"));
        assert!(help.contains("`dress` runs the current backend rehearsal flow"));
        assert!(help.contains("`DRESS_DEPLOYMENT_ROOT`"));
        assert!(help.contains("current working directory as the deployment root"));
    }

    #[test]
    fn help_text_documents_disable_isolation_flag() {
        let help = help_text();

        assert!(help.contains("--disable-isolation"));
        assert!(help.contains("shared state"));
        assert!(help.contains("--yes"));
    }

    #[test]
    fn version_text_uses_package_version() {
        let version = version_text();

        assert_eq!(version, format!("dress {}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn disable_isolation_flag_is_parsed() {
        let selection = select_command(&["dress".to_string(), "--disable-isolation".to_string()])
            .expect("disable-isolation flag should parse");

        assert!(matches!(
            selection,
            CommandSelection::RunBackendRehearsal(RehearsalOptions {
                disable_isolation: true,
                yes: false
            })
        ));
    }

    #[test]
    fn yes_flag_is_parsed_with_disable_isolation() {
        let selection = select_command(&[
            "dress".to_string(),
            "--disable-isolation".to_string(),
            "--yes".to_string(),
        ])
        .expect("yes flag should parse");

        assert!(matches!(
            selection,
            CommandSelection::RunBackendRehearsal(RehearsalOptions {
                disable_isolation: true,
                yes: true
            })
        ));
    }

    #[test]
    fn short_yes_flag_is_parsed() {
        let selection = select_command(&[
            "dress".to_string(),
            "--disable-isolation".to_string(),
            "-y".to_string(),
        ])
        .expect("short yes flag should parse");

        assert!(matches!(
            selection,
            CommandSelection::RunBackendRehearsal(RehearsalOptions {
                disable_isolation: true,
                yes: true
            })
        ));
    }

    #[test]
    fn load_smoke_config_applies_isolated_execution_mode() {
        let environment =
            SmokeEnvironment::default().with_var("DRESS_DEPLOYMENT_ROOT", "/tmp/deploy");

        let config = load_smoke_config(&environment, TerraformExecutionMode::Isolated)
            .expect("config should load");

        assert!(config.backend_config.execution_mode().is_isolated());
    }

    #[test]
    fn load_smoke_config_applies_non_isolated_execution_mode() {
        let environment =
            SmokeEnvironment::default().with_var("DRESS_DEPLOYMENT_ROOT", "/tmp/deploy");

        let config = load_smoke_config(&environment, TerraformExecutionMode::NonIsolated)
            .expect("config should load");

        assert!(!config.backend_config.execution_mode().is_isolated());
    }

    #[test]
    fn failure_with_failed_step_stderr_path_is_warning_colored() {
        let temp_dir =
            TestDir::new("cli-tests", "failure-classifier").expect("temp dir should exist");
        let stderr_path = temp_dir.path().join("terraform.stderr.log");

        assert!(failure_summary_should_use_warning_style(
            "backend `terraform` step failed during deploy: Requested entity already exists",
            None
        ));
        assert!(!failure_summary_should_use_warning_style(
            "backend `terraform` step failed during deploy: exit status: 1",
            None
        ));
        std::fs::write(
            &stderr_path,
            "googleapi: Error 409: Requested entity already exists",
        )
        .expect("stderr fixture should write");
        assert!(
            failed_step_stderr_path_looks_like_existing_resource_conflict(Some(
                stderr_path.as_path()
            ))
        );
        assert!(failure_summary_should_use_warning_style(
            "backend `terraform` step failed during deploy: step `terraform-apply` exited unsuccessfully: exit status: 1",
            Some(stderr_path.as_path())
        ));
        assert!(!failed_step_stderr_path_looks_like_existing_resource_conflict(None));
    }

    #[test]
    fn existing_resource_conflicts_are_classified_by_text() {
        assert!(failure_error_looks_like_existing_resource_conflict(
            "Requested entity already exists"
        ));
        assert!(failure_error_looks_like_existing_resource_conflict(
            "Error 409: resource already exists"
        ));
        assert!(failure_error_looks_like_existing_resource_conflict(
            "EntityAlreadyExists"
        ));
        assert!(!failure_error_looks_like_existing_resource_conflict(
            "exit status: 1"
        ));
    }
}
