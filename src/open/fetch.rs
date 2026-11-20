//! The CLI's first-run download of the local page and its engine.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use flate2::read::MultiGzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{CORE_VERSION, Error, app_error, check_app_api, check_manifest};

const GITHUB: &str = "https://github.com";
const APP_LOCK: &str = include_str!("../../app.lock.json");
const UNPINNED: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const MARKER: &str = ".sha256";
const FILES: &str = ".files.json";
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

/// A fixture for subprocess integration tests. Published builds have no runtime source override.
#[cfg(feature = "asset-test-hooks")]
pub(super) fn loopback_fixture() -> Option<(String, String, PathBuf)> {
    let base = std::env::var("SPECIFY_TEST_APP_BASE_URL").ok()?;
    let cache = PathBuf::from(std::env::var_os("SPECIFY_TEST_CACHE_DIR")?);
    let addr = base
        .strip_prefix("http://")?
        .parse::<std::net::SocketAddrV4>()
        .ok()?;
    if !addr.ip().is_loopback() || !cache.is_absolute() {
        return None;
    }
    let pin = std::env::var("SPECIFY_TEST_APP_PIN").ok()?;
    Some((base, pin, cache))
}

#[cfg(feature = "asset-test-hooks")]
impl<'a> Source<'a> {
    pub(super) fn fixture(base: &'a str, pin: &'a str, cache: &'a Path) -> Self {
        Self {
            base,
            pin,
            cache: Some(cache),
        }
    }
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
        let Ok(marker) = read_metadata(&dir.join(MARKER), 128) else {
            return false;
        };
        let matching = str::from_utf8(&marker).is_ok_and(|marker| {
            let marker = marker.trim();
            self.sha256
                .map_or_else(|| is_sha256(marker), |pin| pin == marker)
        });
        matching && verify_inventory(dir).is_ok() && self.validate(dir).is_ok()
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
            return dest.canonicalize().map_err(cache_error);
        }

        // Remove invalid assets before fetching. Existing readers may see a gap until the
        // replacement is published, including when an offline download fails.
        match fs::symlink_metadata(&dest) {
            Ok(meta) if meta.is_dir() => fs::remove_dir_all(&dest).map_err(cache_error)?,
            Ok(meta) => remove_cache_file(&dest, meta.file_type()).map_err(cache_error)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(cache_error(e)),
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
        let inventory = inventory(&prepared, false).map_err(cache_error)?;
        {
            let mut files =
                io::BufWriter::new(File::create_new(prepared.join(FILES)).map_err(cache_error)?);
            serde_json::to_writer(&mut files, &inventory).map_err(|e| cache_error(e.into()))?;
            files.flush().map_err(cache_error)?;
        }
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(prepared.join(MARKER))
            .and_then(|mut file| writeln!(file, "{digest}"))
            .map_err(cache_error)?;

        fs::rename(&prepared, &dest).map_err(cache_error)?;
        dest.canonicalize().map_err(cache_error)
    }

    fn download(&self, bundle: &Bundle<'_>) -> Result<(Vec<u8>, String), Error> {
        let kind = bundle.kind;
        let url = format!(
            "{}/{}/releases/download/{}/{}",
            self.base, bundle.repository, bundle.tag, bundle.asset
        );
        let agent = download_agent(self.base);
        let _ = writeln!(
            io::stderr(),
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

fn remove_cache_file(path: &Path, _kind: fs::FileType) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileTypeExt;
        if _kind.is_symlink_dir() {
            return fs::remove_dir(path);
        }
    }
    fs::remove_file(path)
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
    hex_digest(&Sha256::digest(bytes))
}

fn hex_digest(digest: &[u8]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn download_agent(base: &str) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .https_only(!base.starts_with("http://"))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(120)));
    let config = if base.starts_with("http://") {
        config.proxy(None)
    } else {
        config
    };
    config.build().new_agent()
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
enum Fingerprint {
    Directory,
    File { sha256: String, size: u64 },
}

type Inventory = BTreeMap<PathBuf, Fingerprint>;

fn read_metadata(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() > limit {
        return Err(io::Error::other("invalid cache metadata"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("cache metadata exceeds size limit"));
    }
    Ok(bytes)
}

// Verification detects corruption and partial or accidentally edited installs. The threat model
// excludes local attackers with write access to the user's cache, who could equally replace the binary.
fn verify_inventory(dir: &Path) -> io::Result<()> {
    let expected: Inventory =
        serde_json::from_slice(&read_metadata(&dir.join(FILES), MAX_ARCHIVE_BYTES)?)?;
    if inventory(dir, true)? != expected {
        return Err(io::Error::other(
            "cache contents differ from the file manifest",
        ));
    }
    Ok(())
}

fn inventory(root: &Path, cached: bool) -> io::Result<Inventory> {
    if !fs::symlink_metadata(root)?.is_dir() {
        return Err(io::Error::other("cache entry is not a directory"));
    }
    let mut files = BTreeMap::new();
    let mut pending = vec![PathBuf::new()];
    let mut remaining = MAX_ARCHIVE_BYTES;
    let mut buffer = [0; 64 * 1024];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(root.join(&dir))? {
            let entry = entry?;
            let name = entry.file_name();
            if cached && dir.as_os_str().is_empty() && (name == MARKER || name == FILES) {
                continue;
            }
            if files.len() >= MAX_ARCHIVE_ENTRIES {
                return Err(io::Error::other("too many cache entries"));
            }
            let path = dir.join(name);
            let kind = entry.file_type()?;
            let fingerprint = if kind.is_dir() {
                pending.push(path.clone());
                Fingerprint::Directory
            } else if kind.is_file() {
                let mut digest = Sha256::new();
                let limit = remaining + 1;
                let mut reader = File::open(entry.path())?.take(limit);
                loop {
                    let len = reader.read(&mut buffer)?;
                    if len == 0 {
                        break;
                    }
                    digest.update(&buffer[..len]);
                }
                let size = limit - reader.limit();
                remaining = remaining
                    .checked_sub(size)
                    .ok_or_else(|| io::Error::other("cache contents exceed size limit"))?;
                Fingerprint::File {
                    sha256: hex_digest(&digest.finalize()),
                    size,
                }
            } else {
                return Err(io::Error::other("unsupported cache entry"));
            };
            files.insert(path, fingerprint);
        }
    }
    Ok(files)
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
    let mut remaining = count_headers(bytes)?;
    let mut archive = tar::Archive::new(MultiGzDecoder::new(bytes).take(MAX_ARCHIVE_BYTES + 1));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        let mut relative = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => (),
                Component::Normal(name) if safe_name(name.as_encoded_bytes()) => {
                    relative.push(name)
                }
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
            create_dirs(dest, &relative, &mut remaining)?;
        } else if kind.is_file()
            && let Some(parent) = relative.parent()
        {
            create_dirs(dest, parent, &mut remaining)?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(dest.join(relative))?;
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
    Ok(())
}

fn count_headers(bytes: &[u8]) -> io::Result<usize> {
    let mut remaining = MAX_ARCHIVE_ENTRIES;
    let mut archive = tar::Archive::new(MultiGzDecoder::new(bytes).take(MAX_ARCHIVE_BYTES + 1));
    for entry in archive.entries()?.raw(true) {
        count_entry(&mut remaining)?;
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        if kind.is_gnu_sparse() {
            return Err(io::Error::other("sparse entries are unsupported"));
        }
        // Both passes must agree on header boundaries. A PAX size override can shift them.
        if (kind.is_pax_local_extensions() || kind.is_pax_global_extensions())
            && let Some(extensions) = entry.pax_extensions()?
        {
            for field in extensions {
                if field?.key_bytes() == b"size" {
                    return Err(io::Error::other("PAX size overrides are unsupported"));
                }
            }
        }
    }
    // Read past the tar end marker to check the gzip trailer and the full unpacked size.
    let mut decoded = archive.into_inner();
    io::copy(&mut decoded, &mut io::sink())?;
    if decoded.limit() == 0 {
        return Err(io::Error::other("unpacked archive exceeds 100 MiB"));
    }
    Ok(remaining)
}

fn count_entry(remaining: &mut usize) -> io::Result<()> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or_else(|| io::Error::other("too many entries in archive"))?;
    Ok(())
}

fn create_dirs(root: &Path, relative: &Path, remaining: &mut usize) -> io::Result<()> {
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => (),
            Ok(_) => return Err(io::Error::other("archive directory conflicts with a file")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                count_entry(remaining)?;
                fs::create_dir(&path)?;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn safe_name(name: &[u8]) -> bool {
    // Exclude alternate separators, drive/stream syntax and Windows-trimmed components.
    !name.iter().any(|b| b"\\:\0".contains(b)) && !name.ends_with(b".") && !name.ends_with(b" ")
}
