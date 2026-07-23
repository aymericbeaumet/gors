use crate::rustc::ExecutableProduct;
use std::ffi::OsString;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

#[cfg(unix)]
mod unix;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishedExecutable {
    path: PathBuf,
    content_hash: String,
    size_bytes: u64,
}

#[derive(Debug)]
pub struct PublicExecutableError {
    message: String,
    source: Option<std::io::Error>,
}

pub fn default_executable_path(source: &Path) -> Result<PathBuf, PublicExecutableError> {
    let name = if source
        .extension()
        .is_some_and(|extension| extension == "go")
    {
        source
            .file_stem()
            .filter(|stem| !stem.is_empty())
            .map(OsString::from)
            .ok_or_else(|| {
                PublicExecutableError::message(format!(
                    "cannot derive an executable name from Go source {}",
                    source.display()
                ))
            })?
    } else {
        let metadata = std::fs::metadata(source).map_err(|source_error| {
            PublicExecutableError::io(
                format!("cannot inspect source selection {}", source.display()),
                source_error,
            )
        })?;
        if !metadata.is_dir() {
            return Err(PublicExecutableError::message(format!(
                "cannot derive an executable name from non-Go file {}",
                source.display()
            )));
        }
        source
            .file_name()
            .filter(|name| !name.is_empty())
            .map(OsString::from)
            .map_or_else(
                || {
                    std::fs::canonicalize(source)
                        .map_err(|source_error| {
                            PublicExecutableError::io(
                                format!(
                                    "cannot resolve source directory {} for executable naming",
                                    source.display()
                                ),
                                source_error,
                            )
                        })?
                        .file_name()
                        .filter(|name| !name.is_empty())
                        .map(OsString::from)
                        .ok_or_else(|| {
                            PublicExecutableError::message(format!(
                                "cannot derive an executable name from source directory {}",
                                source.display()
                            ))
                        })
                },
                Ok,
            )?
    };

    let mut filename = name;
    if !std::env::consts::EXE_SUFFIX.is_empty()
        && Path::new(&filename).extension() != Some(std::ffi::OsStr::new("exe"))
    {
        filename.push(std::env::consts::EXE_SUFFIX);
    }
    Ok(PathBuf::from(filename))
}

pub fn publish_executable(
    source: &ExecutableProduct,
    destination: &Path,
) -> Result<PublishedExecutable, PublicExecutableError> {
    #[cfg(unix)]
    {
        unix::publish(source, destination)
    }
    #[cfg(not(unix))]
    {
        let _ = (source, destination);
        Err(PublicExecutableError::message(
            "atomic public executable publication is unsupported on this platform",
        ))
    }
}

impl PublishedExecutable {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    #[must_use]
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    #[cfg(test)]
    #[must_use]
    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }
}

impl PublicExecutableError {
    pub(super) fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }

    pub(super) fn io(message: impl Into<String>, source: std::io::Error) -> Self {
        Self {
            message: message.into(),
            source: Some(source),
        }
    }
}

impl Display for PublicExecutableError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)?;
        if let Some(source) = &self.source {
            write!(formatter, ": {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PublicExecutableError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf, PublicExecutableError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    std::env::current_dir()
        .map(|current| current.join(path))
        .map_err(|source| PublicExecutableError::io("cannot resolve current directory", source))
}

#[cfg(all(test, unix))]
mod tests;
