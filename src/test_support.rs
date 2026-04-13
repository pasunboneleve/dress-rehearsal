use crate::context::RunId;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub fn new(prefix: &str, name: &str) -> io::Result<Self> {
        let path = env::temp_dir().join(format!(
            "dress-rehearsal-{prefix}-{name}-{}",
            RunId::generate().as_str()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
