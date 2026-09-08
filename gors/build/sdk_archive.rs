//! Host-specific distribution formats for the pinned Go SDK.

use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdkArchiveFormat {
    TarGz,
    Zip,
}

impl SdkArchiveFormat {
    pub fn for_host(host_os: &str) -> Self {
        if host_os == "windows" {
            Self::Zip
        } else {
            Self::TarGz
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::TarGz => "tar.gz",
            Self::Zip => "zip",
        }
    }

    pub fn extract(
        self,
        archive: &Path,
        destination: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file = std::fs::File::open(archive)?;
        match self {
            Self::TarGz => {
                tar::Archive::new(flate2::read::GzDecoder::new(file)).unpack(destination)?;
            }
            Self::Zip => zip::ZipArchive::new(file)?.extract(destination)?,
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn host_selects_the_published_sdk_archive_format() {
        assert_eq!(SdkArchiveFormat::for_host("windows").extension(), "zip");
        assert_eq!(SdkArchiveFormat::for_host("darwin").extension(), "tar.gz");
        assert_eq!(SdkArchiveFormat::for_host("linux").extension(), "tar.gz");
    }

    #[test]
    fn windows_zip_extracts_the_sdk_tree_and_executable_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("sdk.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        for (name, content) in [
            ("go/VERSION", b"go1.26.0\n".as_slice()),
            ("go/bin/go.exe", b"MZ\0go"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
        let extracted = directory.path().join("extracted");
        SdkArchiveFormat::Zip.extract(&archive, &extracted).unwrap();
        assert_eq!(
            std::fs::read(extracted.join("go/VERSION")).unwrap(),
            b"go1.26.0\n"
        );
        assert_eq!(
            std::fs::read(extracted.join("go/bin/go.exe")).unwrap(),
            b"MZ\0go"
        );
    }

    #[test]
    fn unix_tarball_extracts_the_sdk_tree() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("sdk.tar.gz");
        let gzip = flate2::write::GzEncoder::new(
            std::fs::File::create(&archive).unwrap(),
            flate2::Compression::default(),
        );
        let mut tar = tar::Builder::new(gzip);
        let content = b"go1.26.0\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "go/VERSION", content.as_slice())
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        let extracted = directory.path().join("extracted");
        SdkArchiveFormat::TarGz
            .extract(&archive, &extracted)
            .unwrap();
        assert_eq!(
            std::fs::read(extracted.join("go/VERSION")).unwrap(),
            content
        );
    }
}
