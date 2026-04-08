//! VerificationSpec and verification execution belong here.

use crate::scenarios::{ScenarioTarget, ScenarioVerification};
use crate::steps::StepCommand;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
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

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(normalize_header_name(&key.into()), value.into());
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
        VerificationTarget::NamedValue { .. } => return Err(VerificationError::MissingHttpTarget),
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

#[cfg(test)]
mod tests {
    use super::{
        FailureArtifactCapture, FailureArtifactSource, HttpMethod, HttpVerificationResponse,
        RetryPolicy, VerificationAssertion, VerificationFailure, VerificationRequest,
        VerificationSpec, VerificationTarget, evaluate_http_response, http_verification_plan,
        verification_spec_from_scenario,
    };
    use crate::scenarios::{ScenarioTarget, ScenarioVerification};
    use std::path::PathBuf;
    use std::time::Duration;

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
}
