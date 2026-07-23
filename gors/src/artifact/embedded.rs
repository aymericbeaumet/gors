//! Native embedded runtime provider and content-addressed publication.

use std::fmt::{Display, Formatter};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use gors_runtime_abi::{
    ArtifactIdentity, ArtifactSchemaVersion, CURRENT_ARTIFACT_SCHEMA, CompatibilityIdentity,
    ContractIdentity, DataWidth, Endianness, ImplementationHash, ProducerIdentity,
    RuntimeAbiManifest, RuntimeArtifactFormat, RuntimeArtifactManifest, RustRlibCompatibility,
    RustRlibProducer, RustRlibRecordError, TargetCapabilities, TargetCapability, TargetModel,
    TargetModelError,
};

use super::RUNTIME_CRATE_NAME;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/gors_runtime_artifact.rs"));
}

static EMBEDDED_ARTIFACT: LazyLock<EmbeddedRuntimeArtifact> =
    LazyLock::new(load_embedded_artifact_or_abort);
static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Exact compiler recipe used to produce the embedded rlib.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeArtifactProducer {
    provenance: RustRlibProducer,
    rustc_verbose_version: &'static [u8],
    canonical_record: &'static [u8],
}

impl RuntimeArtifactProducer {
    /// Complete byte-for-byte `rustc -vV` output recorded by the producer.
    #[must_use]
    pub const fn rustc_verbose_version(&self) -> &'static [u8] {
        self.rustc_verbose_version
    }

    /// Canonical, path-independent producer provenance record.
    #[must_use]
    pub const fn canonical_record(&self) -> &'static [u8] {
        self.canonical_record
    }

    /// Typed producer recipe, including the exact target model.
    #[must_use]
    pub const fn provenance(&self) -> &RustRlibProducer {
        &self.provenance
    }

    /// Immutable identity of the compiler host and recipe that built the rlib.
    #[must_use]
    pub fn identity(&self) -> ProducerIdentity {
        self.provenance.identity()
    }
}

/// One precompiled runtime provider embedded in a native gors distribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddedRuntimeArtifact {
    manifest: RuntimeArtifactManifest,
    payload: &'static [u8],
    producer: RuntimeArtifactProducer,
    compatibility: RustRlibCompatibility,
    compatibility_record: &'static [u8],
}

impl EmbeddedRuntimeArtifact {
    /// Exact schema-2 provider manifest for the embedded payload.
    #[must_use]
    pub const fn manifest(&self) -> &RuntimeArtifactManifest {
        &self.manifest
    }

    /// Exact rlib bytes named by [`RuntimeArtifactManifest::implementation`].
    #[must_use]
    pub const fn payload(&self) -> &'static [u8] {
        self.payload
    }

    /// Exact rustc and fixed producer recipe used for this payload.
    #[must_use]
    pub const fn producer(&self) -> &RuntimeArtifactProducer {
        &self.producer
    }

    /// Host-neutral target ABI that a consumer must reproduce before linking.
    #[must_use]
    pub const fn compatibility(&self) -> &RustRlibCompatibility {
        &self.compatibility
    }

    /// Verify all embedded identities before the payload crosses a file or
    /// process boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when build metadata, the current runtime contract, or
    /// the exact embedded payload disagree.
    pub fn verify(&self) -> Result<(), RuntimeArtifactError> {
        if self.manifest.schema() != CURRENT_ARTIFACT_SCHEMA {
            return Err(RuntimeArtifactError::UnsupportedSchema {
                found: self.manifest.schema(),
                expected: CURRENT_ARTIFACT_SCHEMA,
            });
        }
        let current_contract = RuntimeAbiManifest::current().identity();
        if self.manifest.contract() != current_contract {
            return Err(RuntimeArtifactError::ContractMismatch {
                embedded: self.manifest.contract(),
                current: current_contract,
            });
        }
        let computed_producer = self.producer.provenance.canonical_bytes();
        if computed_producer != self.producer.canonical_record {
            return Err(RuntimeArtifactError::ProducerRecordMismatch);
        }
        let producer_identity = self.producer.identity();
        let embedded_producer = ProducerIdentity::from_bytes(generated::PRODUCER_IDENTITY);
        if embedded_producer != producer_identity {
            return Err(RuntimeArtifactError::ProducerIdentityMismatch {
                embedded: embedded_producer,
                computed: producer_identity,
            });
        }
        let computed_compatibility = self.compatibility.canonical_bytes();
        if computed_compatibility != self.compatibility_record {
            return Err(RuntimeArtifactError::CompatibilityRecordMismatch);
        }
        let compatibility_identity = self.compatibility.identity();
        if self.manifest.compatibility() != compatibility_identity {
            return Err(RuntimeArtifactError::CompatibilityIdentityMismatch {
                embedded: self.manifest.compatibility(),
                computed: compatibility_identity,
            });
        }
        let computed_implementation = ImplementationHash::sha256(self.payload);
        if self.manifest.implementation() != computed_implementation {
            return Err(RuntimeArtifactError::PayloadMismatch {
                embedded: self.manifest.implementation(),
                computed: computed_implementation,
            });
        }
        let computed_artifact = self.manifest.identity();
        let embedded_artifact = ArtifactIdentity::from_bytes(generated::ARTIFACT_IDENTITY);
        if computed_artifact != embedded_artifact {
            return Err(RuntimeArtifactError::ArtifactIdentityMismatch {
                embedded: embedded_artifact,
                computed: computed_artifact,
            });
        }
        Ok(())
    }

    /// Check whether a file contains this provider's exact payload.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the file cannot be read.
    pub fn verify_materialized(&self, path: &Path) -> Result<bool, RuntimeArtifactError> {
        let payload = std::fs::read(path)
            .map_err(|source| RuntimeArtifactError::io("read runtime artifact", path, source))?;
        Ok(self.manifest.verifies_payload(&payload))
    }

    /// Materialize the rlib below an artifact cache root and return its exact
    /// path for `rustc --extern`.
    ///
    /// The resulting layout is
    /// `<cache_root>/<artifact_identity>/lib__gors_runtime.rlib`. Publication
    /// is atomic, and a corrupt existing cache entry is replaced.
    ///
    /// # Errors
    ///
    /// Returns an error if embedded verification, directory creation, atomic
    /// publication, or final payload verification fails.
    pub fn materialize(&self, cache_root: &Path) -> Result<PathBuf, RuntimeArtifactError> {
        self.verify()?;
        let artifact_dir = cache_root.join(self.manifest.identity().to_string());
        std::fs::create_dir_all(&artifact_dir).map_err(|source| {
            RuntimeArtifactError::io("create runtime artifact directory", &artifact_dir, source)
        })?;
        let lock_path = artifact_dir.join(".materialize.lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| {
                RuntimeArtifactError::io("open runtime artifact lock", &lock_path, source)
            })?;
        lock_file.lock().map_err(|source| {
            RuntimeArtifactError::io("lock runtime artifact directory", &lock_path, source)
        })?;

        let destination = artifact_dir.join(format!("lib{RUNTIME_CRATE_NAME}.rlib"));
        if destination.is_file() && self.verify_materialized(&destination)? {
            return Ok(destination);
        }

        let (temporary_path, mut temporary_file) = create_temporary_file(&artifact_dir)?;
        if let Err(source) = temporary_file.write_all(self.payload) {
            drop(std::fs::remove_file(&temporary_path));
            return Err(RuntimeArtifactError::io(
                "write temporary runtime artifact",
                &temporary_path,
                source,
            ));
        }
        if let Err(source) = temporary_file.sync_all() {
            drop(std::fs::remove_file(&temporary_path));
            return Err(RuntimeArtifactError::io(
                "sync temporary runtime artifact",
                &temporary_path,
                source,
            ));
        }
        drop(temporary_file);

        // Re-check under the artifact-directory lock. A concurrent process may
        // have published the same provider while this process was preparing.
        if destination.is_file() && self.verify_materialized(&destination)? {
            drop(std::fs::remove_file(&temporary_path));
            return Ok(destination);
        }
        if destination.exists()
            && let Err(source) = std::fs::remove_file(&destination)
        {
            drop(std::fs::remove_file(&temporary_path));
            return Err(RuntimeArtifactError::io(
                "replace corrupt runtime artifact",
                &destination,
                source,
            ));
        }
        if let Err(source) = std::fs::rename(&temporary_path, &destination) {
            if destination.is_file() && self.verify_materialized(&destination)? {
                drop(std::fs::remove_file(&temporary_path));
                return Ok(destination);
            }
            drop(std::fs::remove_file(&temporary_path));
            return Err(RuntimeArtifactError::io(
                "publish runtime artifact",
                &destination,
                source,
            ));
        }
        if !self.verify_materialized(&destination)? {
            return Err(RuntimeArtifactError::PublishedPayloadMismatch(destination));
        }
        sync_directory(&artifact_dir)?;
        Ok(destination)
    }
}

/// Return the only runtime provider carried by a native compiler build.
#[must_use]
pub fn embedded_runtime_artifact() -> &'static EmbeddedRuntimeArtifact {
    &EMBEDDED_ARTIFACT
}

/// Failure to validate or publish the native runtime provider.
#[derive(Debug)]
pub enum RuntimeArtifactError {
    InvalidTarget(TargetModelError),
    InvalidProducer(RustRlibRecordError),
    InvalidCompatibility(RustRlibRecordError),
    UnsupportedPointerWidth(u16),
    UnsupportedEndianness(&'static str),
    UnsupportedSchema {
        found: ArtifactSchemaVersion,
        expected: ArtifactSchemaVersion,
    },
    ContractMismatch {
        embedded: ContractIdentity,
        current: ContractIdentity,
    },
    ProducerRecordMismatch,
    ProducerIdentityMismatch {
        embedded: ProducerIdentity,
        computed: ProducerIdentity,
    },
    CompatibilityRecordMismatch,
    CompatibilityIdentityMismatch {
        embedded: CompatibilityIdentity,
        computed: CompatibilityIdentity,
    },
    PayloadMismatch {
        embedded: ImplementationHash,
        computed: ImplementationHash,
    },
    ArtifactIdentityMismatch {
        embedded: ArtifactIdentity,
        computed: ArtifactIdentity,
    },
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    PublishedPayloadMismatch(PathBuf),
}

impl RuntimeArtifactError {
    fn io(action: &'static str, path: &Path, source: std::io::Error) -> RuntimeArtifactError {
        RuntimeArtifactError::Io {
            action,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl Display for RuntimeArtifactError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTarget(error) => write!(formatter, "invalid embedded target: {error}"),
            Self::InvalidProducer(error) => {
                write!(formatter, "invalid embedded rustc producer record: {error}")
            }
            Self::InvalidCompatibility(error) => {
                write!(
                    formatter,
                    "invalid embedded Rust compatibility record: {error}"
                )
            }
            Self::UnsupportedPointerWidth(width) => {
                write!(formatter, "unsupported embedded pointer width: {width}")
            }
            Self::UnsupportedEndianness(endian) => {
                write!(formatter, "unsupported embedded endianness: {endian}")
            }
            Self::UnsupportedSchema { found, expected } => write!(
                formatter,
                "embedded runtime artifact schema {} is unsupported; expected {}",
                found.get(),
                expected.get()
            ),
            Self::ContractMismatch { embedded, current } => write!(
                formatter,
                "embedded runtime contract {embedded} does not match current contract {current}"
            ),
            Self::ProducerRecordMismatch => {
                formatter.write_str("embedded runtime producer record is not canonical")
            }
            Self::ProducerIdentityMismatch { embedded, computed } => write!(
                formatter,
                "embedded runtime producer identity {embedded} does not match {computed}"
            ),
            Self::CompatibilityRecordMismatch => {
                formatter.write_str("embedded runtime compatibility record is not canonical")
            }
            Self::CompatibilityIdentityMismatch { embedded, computed } => write!(
                formatter,
                "embedded runtime compatibility identity {embedded} does not match {computed}"
            ),
            Self::PayloadMismatch { embedded, computed } => write!(
                formatter,
                "embedded runtime implementation hash {embedded} does not match {computed}"
            ),
            Self::ArtifactIdentityMismatch { embedded, computed } => write!(
                formatter,
                "embedded runtime artifact identity {embedded} does not match {computed}"
            ),
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "failed to {action} {}: {source}", path.display()),
            Self::PublishedPayloadMismatch(path) => write!(
                formatter,
                "published runtime artifact does not match its implementation hash: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for RuntimeArtifactError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidTarget(error) => Some(error),
            Self::InvalidProducer(error) => Some(error),
            Self::InvalidCompatibility(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn load_embedded_artifact_or_abort() -> EmbeddedRuntimeArtifact {
    match load_embedded_artifact() {
        Ok(artifact) => artifact,
        Err(error) => {
            eprintln!("invalid build-generated runtime artifact metadata: {error}");
            std::process::abort();
        }
    }
}

fn load_embedded_artifact() -> Result<EmbeddedRuntimeArtifact, RuntimeArtifactError> {
    let pointer_width = match generated::TARGET_POINTER_WIDTH {
        32 => DataWidth::Bits32,
        64 => DataWidth::Bits64,
        width => return Err(RuntimeArtifactError::UnsupportedPointerWidth(width)),
    };
    let endianness = match generated::TARGET_ENDIAN {
        "little" => Endianness::Little,
        "big" => Endianness::Big,
        endian => return Err(RuntimeArtifactError::UnsupportedEndianness(endian)),
    };
    let target = TargetModel::new(generated::TARGET_TRIPLE, pointer_width, endianness)
        .map_err(RuntimeArtifactError::InvalidTarget)?;
    let producer_provenance = RustRlibProducer::new(
        generated::RUSTC_VERBOSE_VERSION,
        generated::TARGET_LIBDIR_RECORD,
        target.clone(),
    )
    .map_err(RuntimeArtifactError::InvalidProducer)?;
    let producer = RuntimeArtifactProducer {
        provenance: producer_provenance,
        rustc_verbose_version: generated::RUSTC_VERBOSE_VERSION,
        canonical_record: generated::PRODUCER_RECORD,
    };
    let compatibility = RustRlibCompatibility::new(
        generated::RUSTC_VERBOSE_VERSION,
        generated::TARGET_LIBDIR_RECORD,
        target.clone(),
    )
    .map_err(RuntimeArtifactError::InvalidCompatibility)?;
    let manifest = RuntimeArtifactManifest::from_parts(
        ArtifactSchemaVersion::new(generated::ARTIFACT_SCHEMA),
        ContractIdentity::from_bytes(generated::CONTRACT_IDENTITY),
        target,
        TargetCapabilities::new([TargetCapability::StandardIo]),
        RuntimeArtifactFormat::RustRlibV1,
        CompatibilityIdentity::from_bytes(generated::COMPATIBILITY_IDENTITY),
        ImplementationHash::from_bytes(generated::IMPLEMENTATION_HASH),
    );
    Ok(EmbeddedRuntimeArtifact {
        manifest,
        payload: generated::PAYLOAD,
        producer,
        compatibility,
        compatibility_record: generated::COMPATIBILITY_RECORD,
    })
}

fn create_temporary_file(
    directory: &Path,
) -> Result<(PathBuf, std::fs::File), RuntimeArtifactError> {
    for _ in 0..64 {
        let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!(
            ".lib{RUNTIME_CRATE_NAME}.rlib.tmp-{}-{sequence}",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(RuntimeArtifactError::io(
                    "create temporary runtime artifact",
                    &path,
                    source,
                ));
            }
        }
    }
    let path = directory.join(format!(".lib{RUNTIME_CRATE_NAME}.rlib.tmp"));
    Err(RuntimeArtifactError::io(
        "create temporary runtime artifact",
        &path,
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary filename sequence exhausted",
        ),
    ))
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<(), RuntimeArtifactError> {
    std::fs::File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|source| {
            RuntimeArtifactError::io("sync runtime artifact directory", directory, source)
        })
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<(), RuntimeArtifactError> {
    Ok(())
}
