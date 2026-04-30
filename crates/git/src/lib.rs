use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RepositorySummary {
    pub work_dir: PathBuf,
    pub git_dir: PathBuf,
    pub branch: Option<String>,
    pub changes: usize,
}

impl RepositorySummary {
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, Error> {
        let repo = gix::discover(path.as_ref()).map_err(|source| Error::Discover {
            path: path.as_ref().to_path_buf(),
            source: source.to_string(),
        })?;

        let branch = repo
            .head_name()
            .map_err(|source| Error::ReadHead(source.to_string()))?
            .map(|name| name.shorten().to_string());

        let changes = repo
            .status(gix::progress::Discard)
            .map_err(|source| Error::Status(source.to_string()))?
            .into_iter(Vec::new())
            .map_err(|source| Error::Status(source.to_string()))?
            .try_fold(0, |count, item| {
                item.map(|_| count + 1)
                    .map_err(|source| Error::Status(source.to_string()))
            })?;

        Ok(Self {
            work_dir: repo
                .workdir()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| repo.path().to_path_buf()),
            git_dir: repo.path().to_path_buf(),
            branch,
            changes,
        })
    }

    pub fn is_clean(&self) -> bool {
        self.changes == 0
    }
}

#[derive(Debug, Clone)]
pub enum Error {
    Discover { path: PathBuf, source: String },
    ReadHead(String),
    Status(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discover { path, source } => {
                write!(
                    f,
                    "failed to discover git repository at {}: {source}",
                    path.display()
                )
            }
            Self::ReadHead(source) => write!(f, "failed to read git HEAD: {source}"),
            Self::Status(source) => write!(f, "failed to read git status: {source}"),
        }
    }
}

impl std::error::Error for Error {}
