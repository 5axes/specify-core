//! The CLI's first-run download of the local page and its engine.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use flate2::read::MultiGzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{CORE_VERSION, Error, app_error, check_app_api, check_manifest};

const GITHUB: &str = "https://github.com";
const APP_LOCK: &str = include_str!("../../app.lock.json");
const UNPINNED: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const MARKER: &str = ".sha256";
const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4096;

#[cfg(test)]
mod tests;

#[derive(Deserialize)]
struct Pin<'a> {
    repository: &'a str,
    tag: &'a str,
    asset: &'a str,
    sha256: &'a str,
    api: u32,
}

/// The internal base URL, pin and cache override allow loopback tests with an isolated cache.
/// Normal invocations always use GitHub, the compiled pin and the platform cache.
pub(super) struct Source<'a> {
    base: &'a str,
    pin: &'a str,
    cache: Option<&'a Path>,
}

#[derive(Clone, Copy)]
enum Kind {
    App,
    Wasm,
}

impl Kind {
    fn cache_dir(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Wasm => "wasm",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::App => "Specify app",
            Self::Wasm => "Specify WASM engine",
        }
    }

    fn error(self, message: impl std::fmt::Display) -> Error {
        match self {
            Self::App => app_error(message),
            Self::Wasm => app_error(format!("{message}: pass --wasm DIR / set SPECIFY_WASM_DIR")),
        }
    }
}

struct Bundle<'a> {
    kind: Kind,
    version: &'a str,
    repository: &'a str,
    tag: &'a str,
    asset: &'a str,
    sha256: Option<&'a str>,
}

impl Bundle<'_> {
    fn has_files(&self, dir: &Path) -> bool {
        match self.kind {
            Kind::App => dir.join("index.html").is_file(),
            Kind::Wasm => {
                dir.join("specify_core_wasm.js").is_file()
                    && dir.join("specify_core_wasm_bg.wasm").is_file()
            }
        }
    }

    fn cached(&self, dir: &Path) -> bool {
        // The engine marker records the digest checked against this release's checksum asset.
        // Trust the local cache on later runs so the engine can start offline.
        fs::read_to_string(dir.join(MARKER)).is_ok_and(|marker| {
            let marker = marker.trim();
            self.sha256
                .map_or_else(|| is_sha256(marker), |pin| pin == marker)
        }) && self.has_files(dir)
    }

    fn validate(&self, dir: &Path) -> Result<(), Error> {
        if let Kind::App = self.kind {
            check_manifest(dir, Some(self.version))?;
        }
        if !self.has_files(dir) {
            return Err(self.kind.error(format!(
                "{} is missing required files for the {}",
                self.asset,
                self.kind.label()
            )));
        }
        Ok(())
    }
}

impl Source<'_> {
    pub(super) fn github() -> Self {
        Self {
            base: GITHUB,
            pin: APP_LOCK,
            cache: None,
        }
    }

    pub(super) fn app(&self) -> Result<PathBuf, Error> {
        let pin: Pin<'_> = serde_json::from_str(self.pin)
            .map_err(|e| app_error(format!("invalid app.lock.json: {e}")))?;
        if pin.tag == "v0.0.0" || pin.sha256 == UNPINNED {
            return Err(app_error("no pinned app yet"));
        }
        check_app_api(pin.api)?;
        let version = pin
            .asset
            .strip_prefix("specify-ui-app-")
            .and_then(|name| name.strip_suffix(".tgz"))
            .filter(|v| {
                v.starts_with(|c: char| c.is_ascii_digit())
                    && !v.ends_with('.')
                    && v.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-+_".contains(&b))
            })
            .ok_or_else(|| app_error("invalid app asset name in app.lock.json"))?;
        if !is_sha256(pin.sha256) {
            return Err(app_error("invalid SHA-256 in app.lock.json"));
        }
        let digest = pin.sha256.to_ascii_lowercase();
        self.install(&Bundle {
            kind: Kind::App,
            version,
            repository: pin.repository,
            tag: pin.tag,
            asset: pin.asset,
            sha256: Some(&digest),
        })
    }

    pub(super) fn wasm(&self) -> Result<PathBuf, Error> {
        self.install(&Bundle {
            kind: Kind::Wasm,
            version: CORE_VERSION,
            repository: "pzfreo/specify-core",
            tag: concat!("v", env!("CARGO_PKG_VERSION")),
            asset: concat!("specify-core-wasm-", env!("CARGO_PKG_VERSION"), ".tgz"),
            sha256: None,
        })
    }

    fn install(&self, bundle: &Bundle<'_>) -> Result<PathBuf, Error> {
        let kind = bundle.kind;
        let root = self
            .cache
            .map(Path::to_path_buf)
            .or_else(user_cache)
            .ok_or_else(|| kind.error("cannot locate the specify-core user cache"))?;
        let parent = root.join(kind.cache_dir());
        let dest = parent.join(bundle.version);
        let cache_error = |e: io::Error| {
            kind.error(format!(
                "cannot use the {} cache at {}: {e}",
                kind.label(),
                dest.display()
            ))
        };
        if bundle.cached(&dest) {
            bundle.validate(&dest)?;
            return dest.canonicalize().map_err(cache_error);
        }

        // Keep the lock file in place. Closing it releases the OS lock, including on exit.
        fs::create_dir_all(&parent).map_err(cache_error)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(parent.join(format!("{}.lock", bundle.version)))
            .map_err(cache_error)?;
        lock.lock().map_err(cache_error)?;
        if bundle.cached(&dest) {
            bundle.validate(&dest)?;
            return dest.canonicalize().map_err(cache_error);
        }

        let stage = tempfile::tempdir_in(&parent).map_err(cache_error)?;
        let (archive, digest) = self.download(bundle)?;
        let content = stage.path().join("content");
        fs::create_dir(&content).map_err(cache_error)?;
        unpack(&archive, &content).map_err(|e| {
            kind.error(format!(
                "cannot unpack {} into {}: {e}",
                bundle.asset,
                dest.display()
            ))
        })?;
        let prepared = match kind {
            Kind::App => content,
            Kind::Wasm => content.join("pkg"),
        };
        bundle.validate(&prepared)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(prepared.join(MARKER))
            .and_then(|mut file| writeln!(file, "{digest}"))
            .map_err(cache_error)?;

        // Publish only a validated, complete directory. Move any stale entry aside first.
        let previous = stage.path().join("previous");
        let moved = match fs::rename(&dest, &previous) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(cache_error(e)),
        };
        if let Err(e) = fs::rename(&prepared, &dest) {
            if moved {
                fs::rename(&previous, &dest).map_err(cache_error)?;
            }
            return Err(cache_error(e));
        }
        dest.canonicalize().map_err(cache_error)
    }

    fn download(&self, bundle: &Bundle<'_>) -> Result<(Vec<u8>, String), Error> {
        let kind = bundle.kind;
        let url = format!(
            "{}/{}/releases/download/{}/{}",
            self.base, bundle.repository, bundle.tag, bundle.asset
        );
        let config = ureq::Agent::config_builder()
            .https_only(self.base == GITHUB)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(120)));
        #[cfg(test)]
        let config = config.proxy(None);
        let agent = config.build().new_agent();
        let _ = writeln!(
            io::stderr().lock(),
            "Downloading the {} {} (first run)…",
            kind.label(),
            bundle.version
        );

        let checksum;
        let expected = if let Some(pin) = bundle.sha256 {
            pin
        } else {
            let bytes = get(&agent, &format!("{url}.sha256"), 4096, kind)?;
            checksum = str::from_utf8(&bytes)
                .ok()
                .and_then(|s| s.split_ascii_whitespace().next())
                .filter(|s| is_sha256(s))
                .ok_or_else(|| kind.error(format!("invalid SHA-256 checksum at {url}.sha256")))?
                .to_ascii_lowercase();
            &checksum
        };
        let bytes = get(&agent, &url, MAX_ARCHIVE_BYTES, kind)?;
        let digest = sha256(&bytes);
        if digest != expected {
            return Err(kind.error(format!(
                "SHA-256 checksum mismatch for {} (expected {expected}, got {digest})",
                bundle.asset
            )));
        }
        Ok((bytes, digest))
    }
}

fn user_cache() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("specify-core");
    #[cfg(windows)]
    let dir = dir.join("cache");
    Some(dir)
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn get(agent: &ureq::Agent, url: &str, limit: u64, kind: Kind) -> Result<Vec<u8>, Error> {
    let failed = |e: &dyn std::fmt::Display| {
        kind.error(format!(
            "could not download {url} (check your internet connection): {e}"
        ))
    };
    let http_error = |status| kind.error(format!("could not download {url}: HTTP {status}"));
    let mut response = agent.get(url).call().map_err(|e| match e {
        ureq::Error::StatusCode(status) => http_error(status),
        _ => failed(&e),
    })?;
    if !response.status().is_success() {
        return Err(http_error(response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| failed(&e))?;
    if bytes.len() as u64 > limit {
        return Err(kind.error(format!("download exceeds {limit} bytes at {url}")));
    }
    Ok(bytes)
}

fn unpack(bytes: &[u8], dest: &Path) -> io::Result<()> {
    let mut archive = tar::Archive::new(MultiGzDecoder::new(bytes).take(MAX_ARCHIVE_BYTES + 1));
    for (index, entry) in archive.entries()?.enumerate() {
        if index >= MAX_ARCHIVE_ENTRIES {
            return Err(io::Error::other("too many entries in archive"));
        }
        let mut entry = entry?;
        let path = entry.path()?;
        let mut target = dest.to_path_buf();
        for component in path.components() {
            match component {
                Component::CurDir => (),
                Component::Normal(name) if safe_name(name.as_encoded_bytes()) => target.push(name),
                _ => {
                    return Err(io::Error::other(format!(
                        "unsafe archive path {}",
                        path.display()
                    )));
                }
            }
        }
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&target)?;
        } else if kind.is_file() && target != dest {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)?;
            if io::copy(&mut entry, &mut file)? != entry.size() {
                return Err(io::Error::other(
                    "truncated entry in archive or unpacked size limit exceeded",
                ));
            }
        } else {
            return Err(io::Error::other(format!(
                "unsupported archive entry {}",
                path.display()
            )));
        }
    }
    // Read past the tar end marker to check the gzip trailer and the full unpacked size.
    let mut decoded = archive.into_inner();
    io::copy(&mut decoded, &mut io::sink())?;
    if decoded.limit() == 0 {
        return Err(io::Error::other("unpacked archive exceeds 100 MiB"));
    }
    Ok(())
}

fn safe_name(name: &[u8]) -> bool {
    // Exclude alternate separators, drive/stream syntax and Windows-trimmed components.
    !name.iter().any(|b| b"\\:\0".contains(b)) && !name.ends_with(b".") && !name.ends_with(b" ")
}
