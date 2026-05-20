use flate2::read::GzDecoder;
use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

static STDLIB_ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/go_stdlib.tar.gz"));

type PackageMap = HashMap<String, Vec<(String, String)>>;

static PACKAGES: OnceLock<PackageMap> = OnceLock::new();

fn load_packages() -> PackageMap {
    let decoder = GzDecoder::new(STDLIB_ARCHIVE);
    let mut archive = tar::Archive::new(decoder);
    let mut packages: PackageMap = HashMap::new();

    let Ok(entries) = archive.entries() else {
        return packages;
    };

    for entry in entries.flatten() {
        let mut entry = entry;
        let path = match entry.path() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        if !path.ends_with(".go") {
            continue;
        }

        let (pkg_path, filename) = match path.rsplit_once('/') {
            Some((dir, file)) => (dir.to_string(), file.to_string()),
            None => continue,
        };

        let mut content = String::new();
        if entry.read_to_string(&mut content).is_ok() {
            packages
                .entry(pkg_path)
                .or_default()
                .push((filename, content));
        }
    }

    packages
}

fn packages() -> &'static PackageMap {
    PACKAGES.get_or_init(load_packages)
}

pub fn package_exists(import_path: &str) -> bool {
    packages().contains_key(import_path)
}

pub fn package_files(import_path: &str) -> Option<Vec<(String, String)>> {
    packages().get(import_path).cloned()
}

pub fn list_packages() -> Vec<String> {
    let mut pkgs: Vec<String> = packages().keys().cloned().collect();
    pkgs.sort();
    pkgs
}
