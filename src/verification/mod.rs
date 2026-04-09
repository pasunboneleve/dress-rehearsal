//! VerificationSpec and verification execution belong here.

use crate::context::RunContext;
use crate::scenarios::{ScenarioTarget, ScenarioVerification};
use crate::steps::{StepCommand, StepError, StepRunner};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationSpec {
    readiness_label: String,
    target: VerificationTarget,
    request: Option<VerificationRequest>,
    assertions: Vec<VerificationAssertion>,
    retry_policy: RetryPolicy,
    failure_artifacts: Vec<FailureArtifactCapture>,
    metadata: BTreeMap<String, String>,
}

impl VerificationSpec {
    pub fn new(
        readiness_label: impl Into<String>,
        target: VerificationTarget,
        request: Option<VerificationRequest>,
    ) -> Self {
        Self {
            readiness_label: readiness_label.into(),
            target,
            request,
            assertions: Vec::new(),
            retry_policy: RetryPolicy::default(),
            failure_artifacts: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn readiness_label(&self) -> &str {
        &self.readiness_label
    }

    pub fn target(&self) -> &VerificationTarget {
        &self.target
    }

    pub fn request(&self) -> Option<&VerificationRequest> {
        self.request.as_ref()
    }

    pub fn assertions(&self) -> &[VerificationAssertion] {
        &self.assertions
    }

    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    pub fn failure_artifacts(&self) -> &[FailureArtifactCapture] {
        &self.failure_artifacts
    }

    pub fn metadata(&self) -> &BTreeMap<String, String> {
        &self.metadata
    }

    pub fn with_assertion(mut self, assertion: VerificationAssertion) -> Self {
        self.assertions.push(assertion);
        self
    }

    pub fn with_request(mut self, request: VerificationRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    pub fn with_failure_artifact(mut self, artifact: FailureArtifactCapture) -> Self {
        self.failure_artifacts.push(artifact);
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationTarget {
    HttpEndpoint { url: String },
    NamedValue { key: String, value: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationRequest {
    method: HttpMethod,
    headers: BTreeMap<String, String>,
    body: Option<String>,
}

impl VerificationRequest {
    pub fn new(method: HttpMethod) -> Self {
        Self {
            method,
            headers: BTreeMap::new(),
            body: None,
        }
    }

    pub fn method(&self) -> &HttpMethod {
        &self.method
    }

    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationAssertion {
    StatusCode { expected: u16 },
    BodyContains { expected_substring: String },
    HeaderEquals { header: String, expected: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    max_attempts: u32,
    delay: Duration,
    timeout: Duration,
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, delay: Duration, timeout: Duration) -> Self {
        Self {
            max_attempts,
            delay,
            timeout,
        }
    }

    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    pub fn delay(&self) -> Duration {
        self.delay
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 10,
            delay: Duration::from_secs(2),
            timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailureArtifactCapture {
    source: FailureArtifactSource,
    destination: PathBuf,
}

impl FailureArtifactCapture {
    pub fn new(source: FailureArtifactSource, destination: impl Into<PathBuf>) -> Self {
        Self {
            source,
            destination: destination.into(),
        }
    }

    pub fn source(&self) -> &FailureArtifactSource {
        &self.source
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FailureArtifactSource {
    File(PathBuf),
    HttpResponseBody,
    StepLog { step_name: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpVerificationPlan {
    request_step: StepCommand,
    retry_policy: RetryPolicy,
    failure_artifacts: Vec<FailureArtifactCapture>,
}

impl HttpVerificationPlan {
    pub fn request_step(&self) -> &StepCommand {
        &self.request_step
    }

    pub fn retry_policy(&self) -> &RetryPolicy {
        &self.retry_policy
    }

    pub fn failure_artifacts(&self) -> &[FailureArtifactCapture] {
        &self.failure_artifacts
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpVerificationResponse {
    status_code: u16,
    headers: BTreeMap<String, String>,
    body: String,
}

impl HttpVerificationResponse {
    pub fn new(status_code: u16) -> Self {
        Self {
            status_code,
            headers: BTreeMap::new(),
            body: String::new(),
        }
    }

    pub fn status_code(&self) -> u16 {
        self.status_code
    }

    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn with_header(mut self, key: impl AsRef<str>, value: impl Into<String>) -> Self {
        self.headers
            .insert(normalize_header_name(key.as_ref()), value.into());
        self
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationReport {
    passed: bool,
    failures: Vec<VerificationFailure>,
}

impl VerificationReport {
    pub fn successful() -> Self {
        Self {
            passed: true,
            failures: Vec::new(),
        }
    }

    pub fn passed(&self) -> bool {
        self.passed
    }

    pub fn failures(&self) -> &[VerificationFailure] {
        &self.failures
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationFailure {
    StatusCode {
        expected: u16,
        actual: u16,
    },
    BodyContains {
        expected_substring: String,
    },
    HeaderEquals {
        header: String,
        expected: String,
        actual: Option<String>,
    },
}

#[derive(Debug)]
pub enum VerificationError {
    MissingHttpTarget,
    MissingHttpRequest,
}

impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHttpTarget => {
                f.write_str("verification spec does not target an HTTP endpoint")
            }
            Self::MissingHttpRequest => {
                f.write_str("verification spec does not define an HTTP request")
            }
        }
    }
}

impl std::error::Error for VerificationError {}

#[derive(Debug)]
pub enum VerificationRunError {
    Spec(VerificationError),
    Step {
        source: StepError,
    },
    ResponseRead {
        operation: &'static str,
        source: io::Error,
    },
    InvalidStatusCode {
        value: String,
    },
    AssertionsFailed {
        report: VerificationReport,
    },
    ArtifactPreservation {
        destination: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for VerificationRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec(source) => write!(f, "{source}"),
            Self::Step { source } => write!(f, "verification step failed: {source}"),
            Self::ResponseRead { operation, source } => {
                write!(f, "failed to {operation} verification response: {source}")
            }
            Self::InvalidStatusCode { value } => {
                write!(
                    f,
                    "verification produced an invalid HTTP status code: `{value}`"
                )
            }
            Self::AssertionsFailed { report } => write!(
                f,
                "verification assertions failed with {} issue(s)",
                report.failures().len()
            ),
            Self::ArtifactPreservation {
                destination,
                source,
            } => write!(
                f,
                "failed to preserve verification artifact `{}`: {source}",
                destination.display()
            ),
        }
    }
}

impl std::error::Error for VerificationRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spec(source) => Some(source),
            Self::Step { source } => Some(source),
            Self::ResponseRead { source, .. } => Some(source),
            Self::ArtifactPreservation { source, .. } => Some(source),
            Self::InvalidStatusCode { .. } | Self::AssertionsFailed { .. } => None,
        }
    }
}

pub fn verification_spec_from_scenario(
    scenario_verification: &ScenarioVerification,
) -> VerificationSpec {
    let target = match scenario_verification.target() {
        ScenarioTarget::HttpEndpoint { url } => {
            VerificationTarget::HttpEndpoint { url: url.clone() }
        }
        ScenarioTarget::NamedOutput { key, value } => VerificationTarget::NamedValue {
            key: key.clone(),
            value: value.clone(),
        },
    };

    let mut spec = VerificationSpec::new(scenario_verification.readiness_label(), target, None);

    for (key, value) in scenario_verification.metadata() {
        spec = spec.with_metadata(key.clone(), value.clone());
    }

    if matches!(
        scenario_verification.target(),
        ScenarioTarget::HttpEndpoint { .. }
    ) {
        spec = spec
            .with_request(VerificationRequest::new(HttpMethod::Get))
            .with_assertion(VerificationAssertion::StatusCode { expected: 200 })
            .with_failure_artifact(FailureArtifactCapture::new(
                FailureArtifactSource::HttpResponseBody,
                "verification/http-response.txt",
            ));
    }

    spec
}

pub fn http_verification_plan(
    verification_spec: &VerificationSpec,
) -> Result<HttpVerificationPlan, VerificationError> {
    let url = match verification_spec.target() {
        VerificationTarget::HttpEndpoint { url } => url,
        _ => return Err(VerificationError::MissingHttpTarget),
    };
    let request = verification_spec
        .request()
        .ok_or(VerificationError::MissingHttpRequest)?;

    let mut command = StepCommand::new("http-verify-request", "/bin/sh");
    let mut script = format!(
        "curl --silent --show-error --location --request {}",
        http_method_name(request.method())
    );

    for (key, value) in request.headers() {
        script.push(' ');
        script.push_str("--header ");
        script.push_str(&shell_quote(&format!("{key}: {value}")));
    }

    if let Some(body) = request.body() {
        script.push(' ');
        script.push_str("--data-raw ");
        script.push_str(&shell_quote(body));
    }

    script.push(' ');
    script.push_str(&shell_quote(url));

    command = command.with_args(["-c".to_string(), script]);

    Ok(HttpVerificationPlan {
        request_step: command,
        retry_policy: verification_spec.retry_policy().clone(),
        failure_artifacts: verification_spec.failure_artifacts().to_vec(),
    })
}

pub fn evaluate_http_response(
    verification_spec: &VerificationSpec,
    response: &HttpVerificationResponse,
) -> Result<VerificationReport, VerificationError> {
    if !matches!(
        verification_spec.target(),
        VerificationTarget::HttpEndpoint { .. }
    ) {
        return Err(VerificationError::MissingHttpTarget);
    }

    let mut failures = Vec::new();
    for assertion in verification_spec.assertions() {
        match assertion {
            VerificationAssertion::StatusCode { expected } => {
                if response.status_code() != *expected {
                    failures.push(VerificationFailure::StatusCode {
                        expected: *expected,
                        actual: response.status_code(),
                    });
                }
            }
            VerificationAssertion::BodyContains { expected_substring } => {
                if !response.body().contains(expected_substring) {
                    failures.push(VerificationFailure::BodyContains {
                        expected_substring: expected_substring.clone(),
                    });
                }
            }
            VerificationAssertion::HeaderEquals { header, expected } => {
                let actual = response
                    .headers()
                    .get(&normalize_header_name(header))
                    .cloned();
                if actual.as_deref() != Some(expected.as_str()) {
                    failures.push(VerificationFailure::HeaderEquals {
                        header: header.clone(),
                        expected: expected.clone(),
                        actual,
                    });
                }
            }
        }
    }

    Ok(VerificationReport {
        passed: failures.is_empty(),
        failures,
    })
}

pub fn execute_verification(
    verification_spec: &VerificationSpec,
    runner: &StepRunner,
    run_context: &RunContext,
) -> Result<VerificationReport, VerificationRunError> {
    match verification_spec.target() {
        VerificationTarget::HttpEndpoint { .. } => {
            execute_http_verification(verification_spec, runner, run_context)
        }
        VerificationTarget::NamedValue { .. } => Ok(VerificationReport::successful()),
    }
}

pub fn execute_http_verification(
    verification_spec: &VerificationSpec,
    runner: &StepRunner,
    run_context: &RunContext,
) -> Result<VerificationReport, VerificationRunError> {
    let plan = http_verification_plan(verification_spec).map_err(VerificationRunError::Spec)?;
    let attempt_count = plan.retry_policy().max_attempts().max(1);
    let attempt_artifacts = HttpAttemptArtifacts::new(run_context);
    fs::create_dir_all(&attempt_artifacts.verification_dir).map_err(|source| {
        VerificationRunError::ResponseRead {
            operation: "create verification artifacts directory",
            source,
        }
    })?;
    let mut last_step_error = None;
    let mut last_report = None;
    let mut last_attempt_number = 1;

    for attempt_index in 0..attempt_count {
        let attempt_number = attempt_index + 1;
        last_attempt_number = attempt_number;
        let request_command =
            http_request_command(plan.request_step(), &attempt_artifacts, attempt_number);
        match runner.run_command(&request_command) {
            Ok(outcome) => {
                last_step_error = None;
                fs::write(attempt_artifacts.stdout_log_path(), outcome.stdout()).map_err(
                    |source| VerificationRunError::ResponseRead {
                        operation: "write verification stdout log",
                        source,
                    },
                )?;
                fs::write(attempt_artifacts.stderr_log_path(), outcome.stderr()).map_err(
                    |source| VerificationRunError::ResponseRead {
                        operation: "write verification stderr log",
                        source,
                    },
                )?;

                let response = read_http_response(&attempt_artifacts, attempt_number)?;
                let report = evaluate_http_response(verification_spec, &response)
                    .map_err(VerificationRunError::Spec)?;
                if report.passed() {
                    return Ok(report);
                }
                last_report = Some(report);
            }
            Err(source) => {
                last_report = None;
                last_step_error = Some(source);
            }
        }

        if attempt_index + 1 < attempt_count {
            thread::sleep(plan.retry_policy().delay());
        }
    }

    preserve_failure_artifacts(
        plan.failure_artifacts(),
        run_context,
        &attempt_artifacts,
        last_attempt_number,
    )?;

    if let Some(report) = last_report {
        return Err(VerificationRunError::AssertionsFailed { report });
    }
    if let Some(source) = last_step_error {
        return Err(VerificationRunError::Step { source });
    }

    Err(VerificationRunError::InvalidStatusCode {
        value: String::new(),
    })
}

fn http_method_name(method: &HttpMethod) -> &'static str {
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
    }
}

fn normalize_header_name(header: &str) -> String {
    header.to_ascii_lowercase()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn http_request_command(
    base_command: &StepCommand,
    attempt_artifacts: &HttpAttemptArtifacts,
    attempt_number: u32,
) -> StepCommand {
    let script = format!(
        "{} --dump-header {} --output {} --write-out '%{{http_code}}'",
        base_command
            .args()
            .get(1)
            .expect("HTTP verification commands include a shell script"),
        shell_quote(
            &attempt_artifacts
                .headers_path(attempt_number)
                .display()
                .to_string()
        ),
        shell_quote(
            &attempt_artifacts
                .body_path(attempt_number)
                .display()
                .to_string()
        ),
    );

    let mut command = StepCommand::new(base_command.name().clone(), base_command.program());
    command = command.with_args(["-c".to_string(), script]);
    if let Some(current_dir) = base_command.current_dir() {
        command = command.with_current_dir(current_dir);
    }
    for (key, value) in base_command.environment() {
        command = command.with_env(key.clone(), value.clone());
    }
    command
}

fn read_http_response(
    attempt_artifacts: &HttpAttemptArtifacts,
    attempt_number: u32,
) -> Result<HttpVerificationResponse, VerificationRunError> {
    let status_text =
        fs::read_to_string(attempt_artifacts.stdout_log_path()).map_err(|source| {
            VerificationRunError::ResponseRead {
                operation: "read verification status code",
                source,
            }
        })?;
    let status_code =
        status_text
            .trim()
            .parse()
            .map_err(|_| VerificationRunError::InvalidStatusCode {
                value: status_text.trim().to_string(),
            })?;
    let body =
        fs::read_to_string(attempt_artifacts.body_path(attempt_number)).map_err(|source| {
            VerificationRunError::ResponseRead {
                operation: "read verification response body",
                source,
            }
        })?;
    let headers =
        fs::read_to_string(attempt_artifacts.headers_path(attempt_number)).map_err(|source| {
            VerificationRunError::ResponseRead {
                operation: "read verification response headers",
                source,
            }
        })?;

    let mut response = HttpVerificationResponse::new(status_code).with_body(body);
    for (key, value) in parse_http_headers(&headers) {
        response = response.with_header(key, value);
    }

    Ok(response)
}

fn parse_http_headers(headers: &str) -> BTreeMap<String, String> {
    let mut parsed_headers = BTreeMap::new();
    for line in headers.lines() {
        if line.starts_with("HTTP/") {
            parsed_headers.clear();
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            parsed_headers.insert(normalize_header_name(key.trim()), value.trim().to_string());
        }
    }
    parsed_headers
}

fn preserve_failure_artifacts(
    artifacts: &[FailureArtifactCapture],
    run_context: &RunContext,
    attempt_artifacts: &HttpAttemptArtifacts,
    attempt_number: u32,
) -> Result<(), VerificationRunError> {
    for artifact in artifacts {
        let source = match artifact.source() {
            FailureArtifactSource::HttpResponseBody => {
                attempt_artifacts.body_path(attempt_number).to_path_buf()
            }
            FailureArtifactSource::StepLog { step_name } if step_name == "http-verify-request" => {
                attempt_artifacts.stdout_log_path().to_path_buf()
            }
            FailureArtifactSource::File(path) => path.clone(),
            FailureArtifactSource::StepLog { .. } => continue,
        };
        if !source.exists() {
            continue;
        }

        run_context
            .preserve_file(&source, artifact.destination())
            .map_err(|source_error| VerificationRunError::ArtifactPreservation {
                destination: artifact.destination().to_path_buf(),
                source: source_error,
            })?;
    }

    Ok(())
}

struct HttpAttemptArtifacts {
    verification_dir: PathBuf,
    stdout_log_path: PathBuf,
    stderr_log_path: PathBuf,
}

impl HttpAttemptArtifacts {
    fn new(run_context: &RunContext) -> Self {
        let verification_dir = run_context.artifact_path("verification");
        let stdout_log_path = verification_dir.join("http-verify-request.stdout.txt");
        let stderr_log_path = verification_dir.join("http-verify-request.stderr.txt");
        Self {
            verification_dir,
            stdout_log_path,
            stderr_log_path,
        }
    }

    fn headers_path(&self, attempt_number: u32) -> PathBuf {
        self.verification_dir.join(format!(
            "http-response-headers-attempt-{attempt_number}.txt"
        ))
    }

    fn body_path(&self, attempt_number: u32) -> PathBuf {
        self.verification_dir
            .join(format!("http-response-body-attempt-{attempt_number}.txt"))
    }

    fn stdout_log_path(&self) -> &Path {
        &self.stdout_log_path
    }

    fn stderr_log_path(&self) -> &Path {
        &self.stderr_log_path
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FailureArtifactCapture, FailureArtifactSource, HttpMethod, HttpVerificationResponse,
        RetryPolicy, VerificationAssertion, VerificationFailure, VerificationRequest,
        VerificationRunError, VerificationSpec, VerificationTarget, evaluate_http_response,
        execute_http_verification, http_verification_plan, verification_spec_from_scenario,
    };
    use crate::context::{RunContext, RunId};
    use crate::scenarios::{ScenarioTarget, ScenarioVerification};
    use crate::steps::StepRunner;
    use std::env;
    use std::fs;
    use std::io::{self, Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> io::Result<Self> {
            let path = env::temp_dir().join(format!(
                "dress-rehearsal-verification-tests-{name}-{}",
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
    fn models_retry_policy_and_failure_artifacts_explicitly() {
        let spec = VerificationSpec::new(
            "service ready",
            VerificationTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string(),
            },
            Some(
                VerificationRequest::new(HttpMethod::Get).with_header("Accept", "application/json"),
            ),
        )
        .with_assertion(VerificationAssertion::StatusCode { expected: 200 })
        .with_assertion(VerificationAssertion::BodyContains {
            expected_substring: "ok".to_string(),
        })
        .with_retry_policy(RetryPolicy::new(
            12,
            Duration::from_secs(1),
            Duration::from_secs(30),
        ))
        .with_failure_artifact(FailureArtifactCapture::new(
            FailureArtifactSource::HttpResponseBody,
            "verification/http-response.txt",
        ))
        .with_metadata("service_name", "dress-service");

        assert_eq!(spec.readiness_label(), "service ready");
        assert_eq!(spec.assertions().len(), 2);
        assert_eq!(spec.retry_policy().max_attempts(), 12);
        assert_eq!(spec.retry_policy().delay(), Duration::from_secs(1));
        assert_eq!(spec.retry_policy().timeout(), Duration::from_secs(30));
        assert_eq!(spec.failure_artifacts().len(), 1);
        assert_eq!(
            spec.failure_artifacts()[0].destination(),
            PathBuf::from("verification/http-response.txt")
        );
        assert_eq!(
            spec.metadata().get("service_name"),
            Some(&"dress-service".to_string())
        );
    }

    #[test]
    fn translates_http_scenario_verification_into_default_http_spec() {
        let scenario_verification = ScenarioVerification::new(
            "ecs ready",
            ScenarioTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string(),
            },
        )
        .with_metadata("region", "us-east-1");

        let spec = verification_spec_from_scenario(&scenario_verification);

        assert_eq!(
            spec.target(),
            &VerificationTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string()
            }
        );
        assert_eq!(
            spec.request().map(VerificationRequest::method),
            Some(&HttpMethod::Get)
        );
        assert_eq!(
            spec.assertions(),
            &[VerificationAssertion::StatusCode { expected: 200 }]
        );
        assert_eq!(spec.failure_artifacts().len(), 1);
        assert_eq!(
            spec.metadata().get("region"),
            Some(&"us-east-1".to_string())
        );
    }

    #[test]
    fn translates_named_output_verification_without_http_defaults() {
        let scenario_verification = ScenarioVerification::new(
            "output ready",
            ScenarioTarget::NamedOutput {
                key: "service_version".to_string(),
                value: "v1".to_string(),
            },
        );

        let spec = verification_spec_from_scenario(&scenario_verification);

        assert_eq!(
            spec.target(),
            &VerificationTarget::NamedValue {
                key: "service_version".to_string(),
                value: "v1".to_string()
            }
        );
        assert_eq!(spec.assertions().len(), 0);
        assert_eq!(spec.failure_artifacts().len(), 0);
        assert_eq!(spec.request(), None);
    }

    #[test]
    fn builds_http_verification_plan_from_http_spec() {
        let spec = VerificationSpec::new(
            "service ready",
            VerificationTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string(),
            },
            Some(
                VerificationRequest::new(HttpMethod::Get).with_header("Accept", "application/json"),
            ),
        )
        .with_retry_policy(RetryPolicy::new(
            5,
            Duration::from_secs(3),
            Duration::from_secs(20),
        ))
        .with_failure_artifact(FailureArtifactCapture::new(
            FailureArtifactSource::HttpResponseBody,
            "verification/http-response.txt",
        ));

        let plan = http_verification_plan(&spec).expect("http plan should be created");

        assert_eq!(plan.request_step().name().as_str(), "http-verify-request");
        assert!(
            plan.request_step()
                .display_command()
                .contains("curl --silent --show-error --location --request GET")
        );
        assert!(
            plan.request_step()
                .display_command()
                .contains("Accept: application/json")
        );
        assert_eq!(plan.retry_policy().max_attempts(), 5);
        assert_eq!(plan.failure_artifacts().len(), 1);
    }

    #[test]
    fn http_verification_plan_uses_data_raw_for_request_bodies() {
        let spec = VerificationSpec::new(
            "service ready",
            VerificationTarget::HttpEndpoint {
                url: "https://service.example.test/submit".to_string(),
            },
            Some(VerificationRequest::new(HttpMethod::Post).with_body("@literal-body")),
        );

        let plan = http_verification_plan(&spec).expect("http plan should be created");

        assert!(plan.request_step().display_command().contains("--data-raw"));
        assert!(
            plan.request_step()
                .display_command()
                .contains("@literal-body")
        );
    }

    #[test]
    fn evaluates_http_assertions_against_response() {
        let spec = VerificationSpec::new(
            "service ready",
            VerificationTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string(),
            },
            Some(VerificationRequest::new(HttpMethod::Get)),
        )
        .with_assertion(VerificationAssertion::StatusCode { expected: 200 })
        .with_assertion(VerificationAssertion::BodyContains {
            expected_substring: "ok".to_string(),
        })
        .with_assertion(VerificationAssertion::HeaderEquals {
            header: "content-type".to_string(),
            expected: "application/json".to_string(),
        });
        let response = HttpVerificationResponse::new(200)
            .with_header("content-type", "application/json")
            .with_body("{\"status\":\"ok\"}");

        let report = evaluate_http_response(&spec, &response).expect("evaluation should succeed");

        assert!(report.passed());
        assert_eq!(report.failures(), &[]);
    }

    #[test]
    fn evaluates_headers_case_insensitively() {
        let spec = VerificationSpec::new(
            "service ready",
            VerificationTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string(),
            },
            Some(VerificationRequest::new(HttpMethod::Get)),
        )
        .with_assertion(VerificationAssertion::HeaderEquals {
            header: "Content-Type".to_string(),
            expected: "application/json".to_string(),
        });
        let response =
            HttpVerificationResponse::new(200).with_header("content-type", "application/json");

        let report = evaluate_http_response(&spec, &response).expect("evaluation should succeed");

        assert!(report.passed());
    }

    #[test]
    fn reports_http_assertion_failures_explicitly() {
        let spec = VerificationSpec::new(
            "service ready",
            VerificationTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string(),
            },
            Some(VerificationRequest::new(HttpMethod::Get)),
        )
        .with_assertion(VerificationAssertion::StatusCode { expected: 200 })
        .with_assertion(VerificationAssertion::BodyContains {
            expected_substring: "ok".to_string(),
        })
        .with_assertion(VerificationAssertion::HeaderEquals {
            header: "content-type".to_string(),
            expected: "application/json".to_string(),
        });
        let response = HttpVerificationResponse::new(503)
            .with_header("content-type", "text/plain")
            .with_body("not ready");

        let report = evaluate_http_response(&spec, &response).expect("evaluation should succeed");

        assert!(!report.passed());
        assert_eq!(
            report.failures(),
            &[
                VerificationFailure::StatusCode {
                    expected: 200,
                    actual: 503,
                },
                VerificationFailure::BodyContains {
                    expected_substring: "ok".to_string(),
                },
                VerificationFailure::HeaderEquals {
                    header: "content-type".to_string(),
                    expected: "application/json".to_string(),
                    actual: Some("text/plain".to_string()),
                },
            ]
        );
    }

    #[test]
    fn executes_http_verification_and_preserves_failure_artifacts() -> io::Result<()> {
        let server = TestHttpServer::respond_once(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Type: text/plain\r\nContent-Length: 9\r\n\r\nnot ready",
        )?;
        let temp_dir = TestDir::new("execute-http-failure")?;
        let run_context =
            RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-verification-0001"));
        run_context.materialize()?;

        let spec = VerificationSpec::new(
            "service ready",
            VerificationTarget::HttpEndpoint {
                url: format!("{}/health", server.base_url()),
            },
            Some(VerificationRequest::new(HttpMethod::Get)),
        )
        .with_assertion(VerificationAssertion::StatusCode { expected: 200 })
        .with_failure_artifact(FailureArtifactCapture::new(
            FailureArtifactSource::HttpResponseBody,
            "verification/http-response.txt",
        ))
        .with_retry_policy(RetryPolicy::new(
            1,
            Duration::from_millis(0),
            Duration::from_secs(5),
        ));

        let error = execute_http_verification(&spec, &StepRunner::new(), &run_context)
            .expect_err("verification should fail");

        match error {
            VerificationRunError::AssertionsFailed { report } => {
                assert_eq!(
                    report.failures(),
                    &[VerificationFailure::StatusCode {
                        expected: 200,
                        actual: 503,
                    }]
                );
            }
            other => panic!("expected assertion failure, got {other:?}"),
        }

        assert_eq!(
            fs::read_to_string(
                run_context
                    .preserved_dir()
                    .join("verification/http-response.txt")
            )?,
            "not ready"
        );

        Ok(())
    }

    struct TestHttpServer {
        address: String,
        join_handle: Option<thread::JoinHandle<()>>,
    }

    impl TestHttpServer {
        fn respond_once(response: &'static str) -> io::Result<Self> {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let address = listener.local_addr()?;
            let join_handle = thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut request = [0_u8; 1024];
                    let _ = stream.read(&mut request);
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
            });

            Ok(Self {
                address: format!("http://{address}"),
                join_handle: Some(join_handle),
            })
        }

        fn base_url(&self) -> &str {
            &self.address
        }
    }

    impl Drop for TestHttpServer {
        fn drop(&mut self) {
            if let Some(join_handle) = self.join_handle.take() {
                let _ = join_handle.join();
            }
        }
    }
}
