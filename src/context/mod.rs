//! RunContext and derived path handling belong here.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static RUN_ID_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Explicit identity for a single rehearsal run.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct RunId(String);

impl RunId {
    /// Generates a process-unique run id using wall clock time plus a sequence.
    pub fn generate() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let sequence = RUN_ID_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self(format!("run-{now:016x}-{sequence:04x}"))
    }

    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Materialized filesystem layout and metadata for one rehearsal run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunContext {
    run_id: RunId,
    root_dir: PathBuf,
    work_dir: PathBuf,
    artifacts_dir: PathBuf,
    preserved_dir: PathBuf,
    metadata_path: PathBuf,
}

impl RunContext {
    /// Builds a new run context rooted under the provided runs directory.
    pub fn new(runs_root: impl Into<PathBuf>) -> Self {
        Self::with_run_id(runs_root, RunId::generate())
    }

    /// Builds a run context with an explicit run id, primarily for tests.
    pub fn with_run_id(runs_root: impl Into<PathBuf>, run_id: RunId) -> Self {
        let runs_root = runs_root.into();
        let root_dir = runs_root.join(run_id.as_str());
        let work_dir = root_dir.join("work");
        let artifacts_dir = root_dir.join("artifacts");
        let preserved_dir = root_dir.join("preserved");
        let metadata_path = root_dir.join("run-metadata.txt");

        Self {
            run_id,
            root_dir,
            work_dir,
            artifacts_dir,
            preserved_dir,
            metadata_path,
        }
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    pub fn artifacts_dir(&self) -> &Path {
        &self.artifacts_dir
    }

    pub fn preserved_dir(&self) -> &Path {
        &self.preserved_dir
    }

    pub fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }

    pub fn artifact_path(&self, relative_path: impl AsRef<Path>) -> PathBuf {
        join_relative_path(&self.artifacts_dir, relative_path)
    }

    pub fn preserved_artifact_path(&self, relative_path: impl AsRef<Path>) -> PathBuf {
        join_relative_path(&self.preserved_dir, relative_path)
    }

    /// Creates the run directory layout so downstream orchestration receives
    /// stable, explicit paths instead of reconstructing them ad hoc.
    pub fn materialize(&self) -> io::Result<()> {
        fs::create_dir_all(&self.work_dir)?;
        fs::create_dir_all(&self.artifacts_dir)?;
        fs::create_dir_all(&self.preserved_dir)?;
        self.write_metadata()
    }

    pub fn metadata(&self) -> RunMetadata {
        RunMetadata {
            run_id: self.run_id.to_string(),
            root_dir: self.root_dir.display().to_string(),
            work_dir: self.work_dir.display().to_string(),
            artifacts_dir: self.artifacts_dir.display().to_string(),
            preserved_dir: self.preserved_dir.display().to_string(),
        }
    }

    pub fn write_metadata(&self) -> io::Result<()> {
        if let Some(parent) = self.metadata_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&self.metadata_path, self.metadata().render())
    }

    /// Copies a failure artifact into the preserved area for post-run diagnosis.
    pub fn preserve_file(
        &self,
        source: impl AsRef<Path>,
        relative_destination: impl AsRef<Path>,
    ) -> io::Result<PathBuf> {
        let destination = self.preserved_artifact_path(relative_destination);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy(source, &destination)?;
        Ok(destination)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunMetadata {
    pub run_id: String,
    pub root_dir: String,
    pub work_dir: String,
    pub artifacts_dir: String,
    pub preserved_dir: String,
}

impl RunMetadata {
    pub fn render(&self) -> String {
        format!(
            "run_id={}\nroot_dir={}\nwork_dir={}\nartifacts_dir={}\npreserved_dir={}\n",
            self.run_id, self.root_dir, self.work_dir, self.artifacts_dir, self.preserved_dir
        )
    }
}

fn join_relative_path(base: &Path, relative_path: impl AsRef<Path>) -> PathBuf {
    let relative_path = relative_path.as_ref();
    assert!(
        !relative_path.is_absolute(),
        "run context paths must remain relative to the run directory"
    );
    assert!(
        !relative_path
            .components()
            .any(|component| matches!(component, Component::ParentDir)),
        "run context paths must not traverse outside the run directory"
    );

    base.join(relative_path)
}

#[cfg(test)]
mod tests {
    use super::{RunContext, RunId};
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
                "dress-rehearsal-tests-{name}-{}",
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
    fn derives_paths_from_run_id() {
        let context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-0001"));

        assert_eq!(context.run_id().as_str(), "run-fixed-0001");
        assert_eq!(
            context.root_dir(),
            PathBuf::from("/tmp/dress-runs/run-fixed-0001")
        );
        assert_eq!(
            context.work_dir(),
            PathBuf::from("/tmp/dress-runs/run-fixed-0001/work")
        );
        assert_eq!(
            context.artifacts_dir(),
            PathBuf::from("/tmp/dress-runs/run-fixed-0001/artifacts")
        );
        assert_eq!(
            context.preserved_dir(),
            PathBuf::from("/tmp/dress-runs/run-fixed-0001/preserved")
        );
        assert_eq!(
            context.metadata_path(),
            PathBuf::from("/tmp/dress-runs/run-fixed-0001/run-metadata.txt")
        );
    }

    #[test]
    fn materialize_creates_layout_and_metadata() -> io::Result<()> {
        let temp_dir = TestDir::new("materialize")?;
        let context = RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-0002"));

        context.materialize()?;

        assert!(context.work_dir().is_dir());
        assert!(context.artifacts_dir().is_dir());
        assert!(context.preserved_dir().is_dir());

        let metadata = fs::read_to_string(context.metadata_path())?;
        assert!(metadata.contains("run_id=run-fixed-0002"));
        assert!(metadata.contains("work_dir="));

        Ok(())
    }

    #[test]
    fn preserves_failure_artifacts_under_preserved_dir() -> io::Result<()> {
        let temp_dir = TestDir::new("preserve-file")?;
        let context = RunContext::with_run_id(temp_dir.path(), RunId::new("run-fixed-0003"));
        let source = temp_dir.path().join("stderr.log");

        fs::write(&source, "captured stderr")?;

        let destination = context.preserve_file(&source, "logs/stderr.log")?;

        assert_eq!(
            destination,
            context.preserved_dir().join("logs").join("stderr.log")
        );
        assert_eq!(fs::read_to_string(destination)?, "captured stderr");

        Ok(())
    }

    #[test]
    fn generated_run_ids_are_unique() {
        let first = RunId::generate();
        let second = RunId::generate();

        assert_ne!(first, second);
        assert!(first.as_str().starts_with("run-"));
        assert!(second.as_str().starts_with("run-"));
    }

    #[test]
    #[should_panic(expected = "run context paths must remain relative to the run directory")]
    fn artifact_path_rejects_absolute_paths() {
        let context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-0004"));

        let _ = context.artifact_path("/tmp/escaped");
    }

    #[test]
    #[should_panic(expected = "run context paths must remain relative to the run directory")]
    fn preserved_artifact_path_rejects_absolute_paths() {
        let context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-0005"));

        let _ = context.preserved_artifact_path("/tmp/escaped");
    }

    #[test]
    #[should_panic(expected = "run context paths must not traverse outside the run directory")]
    fn artifact_path_rejects_parent_traversal() {
        let context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-0006"));

        let _ = context.artifact_path("../escaped");
    }

    #[test]
    #[should_panic(expected = "run context paths must not traverse outside the run directory")]
    fn preserved_artifact_path_rejects_parent_traversal() {
        let context = RunContext::with_run_id("/tmp/dress-runs", RunId::new("run-fixed-0007"));

        let _ = context.preserved_artifact_path("../escaped");
    }
}
