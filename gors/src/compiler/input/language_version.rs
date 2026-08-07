use std::fmt;

/// Canonical Go language version used for source compatibility checks.
///
/// Patch releases do not change the language, so inputs such as `1.21.3` and
/// `go1.21.3` canonicalize to `go1.21`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GoLanguageVersion {
    major: u32,
    minor: u32,
}

impl GoLanguageVersion {
    /// Language version Go assigns to a module whose `go.mod` has no `go`
    /// directive. This compatibility default is fixed and must not advance.
    pub const DEFAULT_MODULE: Self = Self::new(1, 16);

    /// First release in which a file build constraint may select a language
    /// version independently of its package's module version.
    pub const FILE_VERSION_FLOOR: Self = Self::new(1, 21);

    /// Construct a language version from canonical numeric components.
    #[must_use]
    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Parse a module (`1.22`) or compiler (`go1.22`) spelling.
    ///
    /// One optional patch component is accepted and discarded because it does
    /// not participate in Go language-version ordering.
    pub fn parse(value: &str) -> Result<Self, GoLanguageVersionParseError> {
        let version = value.strip_prefix("go").unwrap_or(value);
        let mut components = version.split('.');
        let major = parse_component(components.next(), value)?;
        let minor = parse_component(components.next(), value)?;
        if let Some(patch) = components.next() {
            parse_component(Some(patch), value)?;
        }
        if components.next().is_some() || major == 0 {
            return Err(GoLanguageVersionParseError::new(value));
        }
        Ok(Self { major, minor })
    }

    /// Parse the compiler's pinned Go language version.
    pub fn current() -> Result<Self, GoLanguageVersionParseError> {
        Self::parse(crate::GO_VERSION)
    }

    /// Major language-version component.
    #[must_use]
    pub const fn major(self) -> u32 {
        self.major
    }

    /// Minor language-version component.
    #[must_use]
    pub const fn minor(self) -> u32 {
        self.minor
    }
}

impl fmt::Display for GoLanguageVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "go{}.{}", self.major, self.minor)
    }
}

/// Invalid spelling at a compiler-owned Go language-version boundary.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GoLanguageVersionParseError {
    value: String,
}

impl GoLanguageVersionParseError {
    fn new(value: &str) -> Self {
        Self {
            value: value.to_owned(),
        }
    }

    /// Rejected version spelling.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for GoLanguageVersionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid Go language version {:?}; expected 1.N or go1.N with an optional patch release",
            self.value
        )
    }
}

impl std::error::Error for GoLanguageVersionParseError {}

fn parse_component(
    component: Option<&str>,
    complete: &str,
) -> Result<u32, GoLanguageVersionParseError> {
    let Some(component) = component else {
        return Err(GoLanguageVersionParseError::new(complete));
    };
    if component.is_empty()
        || (component.len() > 1 && component.starts_with('0'))
        || !component.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(GoLanguageVersionParseError::new(complete));
    }
    component
        .parse::<u32>()
        .map_err(|_| GoLanguageVersionParseError::new(complete))
}
