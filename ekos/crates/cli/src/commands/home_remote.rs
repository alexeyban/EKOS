//! RFC 0232 — `ekos home backup --to <url>` and `ekos home restore --from <url>`: backups kept in
//! object storage (Amazon S3, Azure Blob Storage, Google Cloud Storage).
//!
//! The archive is the one RFC 0183 writes; this module only moves the file. A URL names a
//! *location* (`s3://bucket/prefix/`, `https://<account>.blob.core.windows.net/<container>/prefix/`,
//! `gs://bucket/prefix/`); a backup is uploaded under it by its own file name, and a restore from
//! a location ending in `/` takes the newest `ekos-backup-*.tar.zst` there.
//!
//! Credentials are never part of a URL or a flag. They come from the machine's cloud identity
//! (instance metadata, a container's managed identity, workload identity) or the standard
//! `AWS_*` / `AZURE_*` / `GOOGLE_*` variables, read by `ObjectStoreBackend::parse_url`.
//!
//! Built only with the `object-storage` feature; without it every entry point says so.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

/// Prefix and suffix of the archive names `ekos home backup` writes; they sort by time.
pub const ARCHIVE_PREFIX: &str = "ekos-backup-";
pub const ARCHIVE_SUFFIX: &str = ".tar.zst";

/// Where an object goes or comes from, split into the store's location and the object's key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The URL with its key removed: what `parse_url` opens.
    pub location: String,
    /// The object's key under `location`, or `None` for "the newest backup there".
    pub object: Option<String>,
}

/// Whether `s` is meant as an object-storage URL rather than a local path.
pub fn is_remote(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    [
        "s3://", "s3a://", "gs://", "az://", "abfs://", "abfss://", "adl://",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
        || (lower.starts_with("https://")
            && (lower.contains(".blob.core.windows.net")
                || lower.contains(".dfs.core.windows.net")
                || lower.contains(".amazonaws.com")
                || lower.contains("storage.googleapis.com")))
}

/// Split a restore URL: one ending in `/` is a location (newest backup), anything else names one
/// archive whose file name must look like a backup.
pub fn parse_target(url: &str) -> Result<Target> {
    if !is_remote(url) {
        bail!(
            "{url:?} is not an object-storage URL (s3://…, gs://…, \
             https://<account>.blob.core.windows.net/…)"
        );
    }
    if url.contains('?') || url.contains('#') {
        bail!("an object-storage URL takes no query or fragment; credentials never go in a URL");
    }
    if url.ends_with('/') {
        return Ok(Target {
            location: url.to_string(),
            object: None,
        });
    }
    let (location, name) = url.rsplit_once('/').unwrap_or(("", url));
    if !is_archive_name(name) {
        bail!(
            "{name:?} is not a backup's name ({ARCHIVE_PREFIX}<time>{ARCHIVE_SUFFIX}); \
             end the URL with `/` to take the newest one there"
        );
    }
    Ok(Target {
        location: format!("{location}/"),
        object: Some(name.to_string()),
    })
}

/// A name `ekos home backup` could have written, and nothing that leaves its directory.
pub fn is_archive_name(name: &str) -> bool {
    name.starts_with(ARCHIVE_PREFIX)
        && name.ends_with(ARCHIVE_SUFFIX)
        && !name.contains('/')
        && !name.contains("..")
}

/// The newest backup among `names` (keys relative to the location). Names carry their time in a
/// sortable form, so the largest name is the newest.
pub fn newest<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    names.into_iter().filter(|n| is_archive_name(n)).max()
}

#[cfg(not(feature = "object-storage"))]
fn unsupported() -> anyhow::Error {
    anyhow::anyhow!(
        "this `ekos` was built without object storage (the `object-storage` feature); the \
         container image has it. Write the backup to a file and copy it with your cloud's tool"
    )
}

/// Whether this build can reach object storage at all; checked before a backup is written.
pub fn available() -> Result<()> {
    #[cfg(feature = "object-storage")]
    {
        Ok(())
    }
    #[cfg(not(feature = "object-storage"))]
    {
        Err(unsupported())
    }
}

/// Upload `file` under `location`; returns the object's full URL.
#[cfg(not(feature = "object-storage"))]
pub fn upload(_file: &Path, _location: &str) -> Result<String> {
    Err(unsupported())
}

/// Download the backup `target` names into `dir`; returns the local file.
#[cfg(not(feature = "object-storage"))]
pub fn download(_target: &Target, _dir: &Path) -> Result<PathBuf> {
    Err(unsupported())
}

#[cfg(feature = "object-storage")]
mod imp {
    use super::*;
    use anyhow::Context;
    use ekos_segment_backend::ObjectStoreBackend;
    use ekos_segment_backend::object_store::path::Path as ObjPath;
    use ekos_segment_backend::object_store::{ObjectStore, ObjectStoreExt, WriteMultipart};
    use futures::StreamExt;
    use std::io::{Read, Write};

    /// Parts are uploaded 8 MiB at a time, four at once.
    const CHUNK: usize = 8 * 1024 * 1024;
    const CONCURRENCY: usize = 4;

    /// Run `fut` to completion. The `ekos` binary is `#[tokio::main]`, and starting a second
    /// runtime inside it panics, so reuse the running one when there is one (the bridge
    /// `commit.rs` uses for embeddings).
    fn block_on<T>(fut: impl std::future::Future<Output = Result<T>>) -> Result<T> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("starting the object-storage runtime")?
                .block_on(fut),
        }
    }

    fn key(prefix: &str, name: &str) -> ObjPath {
        let prefix = prefix.trim_matches('/');
        if prefix.is_empty() {
            ObjPath::from(name)
        } else {
            ObjPath::from(format!("{prefix}/{name}"))
        }
    }

    fn open(location: &str) -> Result<(Box<dyn ObjectStore>, String)> {
        ObjectStoreBackend::parse_url(location)
            .map_err(|e| anyhow::anyhow!("opening {location}: {e}"))
    }

    pub fn upload(file: &Path, location: &str) -> Result<String> {
        if !is_remote(location) {
            bail!("{location:?} is not an object-storage URL");
        }
        upload_unchecked(file, location)
    }

    /// `upload` without the cloud-URL check, so a test can use the `file://` store.
    pub(super) fn upload_unchecked(file: &Path, location: &str) -> Result<String> {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .context("the archive has no file name")?
            .to_string();
        let location = if location.ends_with('/') {
            location.to_string()
        } else {
            format!("{location}/")
        };
        let (store, prefix) = open(&location)?;
        let path = key(&prefix, &name);
        let mut source =
            std::fs::File::open(file).with_context(|| format!("reading {}", file.display()))?;
        block_on(async {
            let upload = store
                .put_multipart(&path)
                .await
                .with_context(|| format!("starting the upload to {location}{name}"))?;
            let mut writer = WriteMultipart::new_with_chunk_size(upload, CHUNK);
            let mut buf = vec![0u8; CHUNK];
            loop {
                let n = source.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                writer.wait_for_capacity(CONCURRENCY).await?;
                writer.write(&buf[..n]);
            }
            writer
                .finish()
                .await
                .with_context(|| format!("finishing the upload to {location}{name}"))?;
            anyhow::Ok(())
        })?;
        Ok(format!("{location}{name}"))
    }

    pub fn download(target: &Target, dir: &Path) -> Result<PathBuf> {
        let (store, prefix) = open(&target.location)?;
        let name = match &target.object {
            Some(n) => n.clone(),
            None => {
                let base = if prefix.trim_matches('/').is_empty() {
                    None
                } else {
                    Some(ObjPath::from(prefix.trim_matches('/')))
                };
                let names: Vec<String> = block_on(async {
                    let mut out = Vec::new();
                    let mut listing = store.list(base.as_ref());
                    while let Some(meta) = listing.next().await {
                        let meta = meta.with_context(|| format!("listing {}", target.location))?;
                        // Only objects directly under the location, not in a sub-prefix.
                        let rel = match &base {
                            Some(b) => meta
                                .location
                                .as_ref()
                                .strip_prefix(b.as_ref())
                                .map(|r| r.trim_start_matches('/').to_string()),
                            None => Some(meta.location.as_ref().to_string()),
                        };
                        if let Some(rel) = rel.filter(|r| !r.contains('/')) {
                            out.push(rel);
                        }
                    }
                    anyhow::Ok(out)
                })?;
                newest(names.iter().map(String::as_str))
                    .with_context(|| format!("no backup under {}", target.location))?
                    .to_string()
            }
        };
        std::fs::create_dir_all(dir)?;
        let dest = dir.join(&name);
        let path = key(&prefix, &name);
        let mut out =
            std::fs::File::create(&dest).with_context(|| format!("writing {}", dest.display()))?;
        block_on(async {
            let mut stream = store
                .get(&path)
                .await
                .with_context(|| format!("reading {}{name}", target.location))?
                .into_stream();
            while let Some(chunk) = stream.next().await {
                out.write_all(&chunk?)?;
            }
            out.sync_all()?;
            anyhow::Ok(())
        })?;
        Ok(dest)
    }
}

#[cfg(feature = "object-storage")]
pub use imp::{download, upload};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_clouds_are_remote_and_a_path_is_not() {
        for url in [
            "s3://bucket/ekos/",
            "gs://bucket/ekos/",
            "https://acct.blob.core.windows.net/backups/ekos/",
            "abfss://backups@acct.dfs.core.windows.net/ekos/",
        ] {
            assert!(is_remote(url), "{url}");
        }
        for path in [
            "/var/lib/ekos/backups/x.tar.zst",
            "backups/x",
            "https://example.com/x",
        ] {
            assert!(!is_remote(path), "{path}");
        }
    }

    #[test]
    fn a_location_takes_the_newest_and_an_object_must_be_a_backup() {
        let t = parse_target("s3://b/ekos/").unwrap();
        assert_eq!(t.location, "s3://b/ekos/");
        assert_eq!(t.object, None);

        let t = parse_target("gs://b/ekos/ekos-backup-20261030T120000Z.tar.zst").unwrap();
        assert_eq!(t.location, "gs://b/ekos/");
        assert_eq!(
            t.object.as_deref(),
            Some("ekos-backup-20261030T120000Z.tar.zst")
        );

        assert!(parse_target("s3://b/ekos/other.txt").is_err());
        assert!(parse_target("s3://b/ekos/?sig=secret").is_err());
        assert!(parse_target("/local/ekos-backup-1.tar.zst").is_err());
    }

    #[test]
    fn newest_is_the_largest_backup_name_and_ignores_other_objects() {
        let names = [
            "ekos-backup-20261029T020000Z.tar.zst",
            "ekos-backup-20261030T020000Z.tar.zst",
            "zzz-not-a-backup.tar.zst",
            "ekos-backup-20261028T020000Z.tar.zst",
        ];
        assert_eq!(
            newest(names.iter().copied()),
            Some("ekos-backup-20261030T020000Z.tar.zst")
        );
        assert_eq!(newest(["notes.txt"].iter().copied()), None);
    }
}

#[cfg(all(test, feature = "object-storage"))]
mod store_tests {
    use super::*;

    /// A round trip through a real `object_store` (the local-filesystem one, `file://`): upload
    /// two backups, restore "the newest under the location", get the second one's bytes back.
    #[test]
    fn upload_then_download_newest_round_trips_through_object_store() {
        let bucket = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let location = format!("file://{}/ekos/", bucket.path().display());
        std::fs::create_dir_all(bucket.path().join("ekos")).unwrap();

        let older = work.path().join("ekos-backup-20261029T020000Z.tar.zst");
        let newer = work.path().join("ekos-backup-20261030T020000Z.tar.zst");
        std::fs::write(&older, b"old").unwrap();
        // Larger than one part, so the multipart path is exercised.
        let big: Vec<u8> = (0..(9 * 1024 * 1024)).map(|i| (i % 251) as u8).collect();
        std::fs::write(&newer, &big).unwrap();

        // `file://` is not a cloud URL, so `upload`'s check is skipped; the transfer is the same.
        let url = imp::upload_unchecked(&older, &location).unwrap();
        assert!(
            url.ends_with("/ekos/ekos-backup-20261029T020000Z.tar.zst"),
            "{url}"
        );
        imp::upload_unchecked(&newer, &location).unwrap();
        // An object in a sub-prefix is not "under the location".
        std::fs::create_dir_all(bucket.path().join("ekos/old")).unwrap();
        std::fs::write(
            bucket
                .path()
                .join("ekos/old/ekos-backup-20991231T000000Z.tar.zst"),
            b"nested",
        )
        .unwrap();

        let got = download(
            &Target {
                location: location.clone(),
                object: None,
            },
            &work.path().join("restore"),
        )
        .unwrap();
        assert_eq!(
            got.file_name().unwrap(),
            "ekos-backup-20261030T020000Z.tar.zst"
        );
        assert_eq!(std::fs::read(got).unwrap(), big);
    }
}
