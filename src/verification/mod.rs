//! VerificationSpec and verification execution belong here.

use crate::scenarios::{ScenarioTarget, ScenarioVerification};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationSpec {
    readiness_label: String,
    target: VerificationTarget,
    request: VerificationRequest,
    assertions: Vec<VerificationAssertion>,
    retry_policy: RetryPolicy,
    failure_artifacts: Vec<FailureArtifactCapture>,
    metadata: BTreeMap<String, String>,
}

impl VerificationSpec {
    pub fn new(
        readiness_label: impl Into<String>,
        target: VerificationTarget,
        request: VerificationRequest,
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

    pub fn request(&self) -> &VerificationRequest {
        &self.request
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

#[derive(Debug)]
pub enum VerificationError {
    InvalidScenarioTarget { message: String },
}

impl fmt::Display for VerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScenarioTarget { message } => {
                write!(f, "invalid verification target: {message}")
            }
        }
    }
}

impl std::error::Error for VerificationError {}

pub fn verification_spec_from_scenario(
    scenario_verification: &ScenarioVerification,
) -> Result<VerificationSpec, VerificationError> {
    let target = match scenario_verification.target() {
        ScenarioTarget::HttpEndpoint { url } => {
            VerificationTarget::HttpEndpoint { url: url.clone() }
        }
        ScenarioTarget::NamedOutput { key, value } => VerificationTarget::NamedValue {
            key: key.clone(),
            value: value.clone(),
        },
    };

    let mut spec = VerificationSpec::new(
        scenario_verification.readiness_label(),
        target,
        VerificationRequest::new(HttpMethod::Get),
    );

    for (key, value) in scenario_verification.metadata() {
        spec = spec.with_metadata(key.clone(), value.clone());
    }

    if matches!(
        scenario_verification.target(),
        ScenarioTarget::HttpEndpoint { .. }
    ) {
        spec = spec
            .with_assertion(VerificationAssertion::StatusCode { expected: 200 })
            .with_failure_artifact(FailureArtifactCapture::new(
                FailureArtifactSource::HttpResponseBody,
                "verification/http-response.txt",
            ));
    }

    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::{
        FailureArtifactCapture, FailureArtifactSource, HttpMethod, RetryPolicy,
        VerificationAssertion, VerificationRequest, VerificationSpec, VerificationTarget,
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
            VerificationRequest::new(HttpMethod::Get).with_header("Accept", "application/json"),
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

        let spec = verification_spec_from_scenario(&scenario_verification)
            .expect("http scenario verification should translate");

        assert_eq!(
            spec.target(),
            &VerificationTarget::HttpEndpoint {
                url: "https://service.example.test/health".to_string()
            }
        );
        assert_eq!(spec.request().method(), &HttpMethod::Get);
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

        let spec = verification_spec_from_scenario(&scenario_verification)
            .expect("named output verification should translate");

        assert_eq!(
            spec.target(),
            &VerificationTarget::NamedValue {
                key: "service_version".to_string(),
                value: "v1".to_string()
            }
        );
        assert_eq!(spec.assertions().len(), 0);
        assert_eq!(spec.failure_artifacts().len(), 0);
    }
}
