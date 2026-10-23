use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::dist::Dist;
use crate::ustar::{self, TarError};

pub const DICT_SIZE: u32 = 1 << 26;

pub trait Source {
    fn fetch(&self, url: &str, sha256: &str, into: &Path) -> Result<PathBuf, String>;
}

pub struct Net;

impl Source for Net {
    fn fetch(&self, url: &str, sha256: &str, into: &Path) -> Result<PathBuf, String> {
        if !url.starts_with("https://") {
            return Err(format!("{url} is not an https:// URL"));
        }
        download(&fetch::Fetcher::new(into), url, sha256)
    }
}

fn download(fetcher: &fetch::Fetcher, url: &str, sha256: &str) -> Result<PathBuf, String> {
    let request = fetch::Request {
        url: url.to_string(),
        dest_rel: "archive".to_string(),
        sha256: sha256.to_string(),
        size_hint: None,
    };
    fetcher
        .fetch_request(&request, &fetch::CancelToken::new(), |_| {})
        .map_err(|e| match e {
            fetch::FetchError::ChecksumMismatch { expected, got } => mismatch(url, &got, &expected),
            fetch::FetchError::Http { status, .. } => {
                format!("could not download {url}: HTTP {status}")
            }
            fetch::FetchError::Offline { .. } => format!("could not reach {url}"),
            fetch::FetchError::Stalled { .. } => format!("{url} stalled"),
            e => format!("could not download {url}: {e}"),
        })
}

fn mismatch(url: &str, got: &str, pinned: &str) -> String {
    format!("{url} has sha256 {got}, not the pinned {pinned}")
}

struct Tidy {
    work: Option<PathBuf>,
    parent: PathBuf,
    made: Option<PathBuf>,
    landed: bool,
}

impl Drop for Tidy {
    fn drop(&mut self) {
        if let Some(work) = &self.work {
            let _ = fs::remove_dir_all(work);
        }
        let Some(made) = self.made.as_deref().filter(|_| !self.landed) else {
            return;
        };
        for dir in self.parent.ancestors() {
            if fs::remove_dir(dir).is_err() || dir == made {
                break;
            }
        }
    }
}

fn topmost_missing(dir: &Path) -> Option<PathBuf> {
    let mut made = None;
    let mut d = dir;
    while fs::symlink_metadata(d).is_err() {
        made = Some(d.to_path_buf());
        match d.parent() {
            Some(up) if !up.as_os_str().is_empty() => d = up,
            _ => break,
        }
    }
    made
}

fn temp_dir(parent: &Path) -> io::Result<PathBuf> {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = parent.join(format!(".ort-cache.{}.{nanos:x}.{n}", std::process::id()));
    fs::create_dir(&dir).map(|()| dir)
}

pub fn sha256_of(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = fetch::StreamHasher::new();
    let mut buf = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(hasher.finalize_hex());
        }
        hasher.update(&buf[..n]);
    }
}

pub fn unpack<R: Read>(archive: R, files: &Path, url: &str) -> Result<(), String> {
    let not_lzma2 = || format!("{url} is not a raw LZMA2 stream");
    let mut stream = lzma_rust2::Lzma2Reader::new(archive, DICT_SIZE, None);
    let count = match ustar::extract(&mut stream, files) {
        Ok(count) => count,
        Err(TarError::Read(_)) => return Err(not_lzma2()),
        Err(TarError::NotPlain(name)) => {
            return Err(format!(
                "{url} holds '{name}', which is not a plain file at the top level"
            ));
        }
        Err(TarError::Twice(name)) => return Err(format!("{url} holds '{name}' twice")),
        Err(TarError::Corrupt(why)) => {
            return Err(format!("{url} is not a readable archive: {why}"));
        }
        Err(TarError::Write(path, e)) => return Err(format!("cannot write {path}: {e}")),
    };
    if io::copy(&mut stream, &mut io::sink()).is_err() {
        return Err(not_lzma2());
    }
    if count == 0 {
        return Err(format!("{url} is an empty archive"));
    }
    Ok(())
}

pub fn land(source: &dyn Source, dist: &Dist, dest: &Path, lib: &str) -> Result<(), String> {
    let url = dist.url.as_str();
    let parent = dest
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", dest.display()))?;
    let mut tidy = Tidy {
        work: None,
        parent: parent.to_path_buf(),
        made: topmost_missing(parent),
        landed: false,
    };
    fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let work = temp_dir(parent)
        .map_err(|e| format!("cannot create a temp folder in {}: {e}", parent.display()))?;
    tidy.work = Some(work.clone());

    let archive = source.fetch(url, &dist.sha256, &work)?;
    let got = sha256_of(&archive).map_err(|e| format!("cannot read {}: {e}", archive.display()))?;
    if got != dist.sha256 {
        return Err(mismatch(url, &got, &dist.sha256));
    }

    let files = work.join("files");
    fs::create_dir(&files).map_err(|e| format!("cannot create {}: {e}", files.display()))?;
    let reader =
        File::open(&archive).map_err(|e| format!("cannot read {}: {e}", archive.display()))?;
    unpack(BufReader::new(reader), &files, url)?;
    if !files.join(lib).is_file() {
        return Err(format!("{url} holds no {lib}"));
    }

    if let Err(e) = fs::rename(&files, dest)
        && !dest.join(lib).is_file()
    {
        return Err(format!(
            "cannot move the files into {}: {e}",
            dest.display()
        ));
    }
    tidy.landed = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;
    use std::cell::Cell;

    const URL: &str = "https://example.invalid/ort.tar.lzma2";

    fn scratch() -> tempfile::TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp/testscratch");
        fs::create_dir_all(&root).unwrap();
        tempfile::Builder::new()
            .prefix("land-")
            .tempdir_in(root)
            .unwrap()
    }

    struct Bytes {
        data: Vec<u8>,
        calls: Cell<usize>,
        then: Box<dyn Fn()>,
    }

    impl Bytes {
        fn new(data: Vec<u8>) -> Self {
            Self {
                data,
                calls: Cell::new(0),
                then: Box::new(|| {}),
            }
        }
    }

    impl Source for Bytes {
        fn fetch(&self, _: &str, _: &str, into: &Path) -> Result<PathBuf, String> {
            self.calls.set(self.calls.get() + 1);
            let path = into.join("archive");
            fs::write(&path, &self.data).unwrap();
            (self.then)();
            Ok(path)
        }
    }

    struct Fails;

    impl Source for Fails {
        fn fetch(&self, url: &str, _: &str, _: &Path) -> Result<PathBuf, String> {
            Err(format!("could not download {url}: offline"))
        }
    }

    fn dist_for(data: &[u8]) -> Dist {
        Dist {
            url: URL.into(),
            sha256: fetch::sha256_hex(data),
        }
    }

    fn good() -> Vec<u8> {
        fixture::lzma2(&fixture::tar(&[
            "f:libonnxruntime.a",
            "f:libwebgpu_dawn.so",
        ]))
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut v: Vec<_> = fs::read_dir(dir)
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn a_good_archive_lands_its_files_and_nothing_else() {
        let s = scratch();
        let dest = s.path().join("cache/dfbin/t/h");
        let data = good();
        land(
            &Bytes::new(data.clone()),
            &dist_for(&data),
            &dest,
            "libonnxruntime.a",
        )
        .unwrap();
        assert_eq!(entries(&dest), ["libonnxruntime.a", "libwebgpu_dawn.so"]);
        assert_eq!(
            fs::read(dest.join("libonnxruntime.a")).unwrap(),
            b"libonnxruntime.a"
        );
        assert_eq!(entries(dest.parent().unwrap()), ["h"]);
    }

    #[test]
    fn a_wrong_sha256_is_refused_before_any_extraction_and_leaves_nothing() {
        let s = scratch();
        let dest = s.path().join("cache/dfbin/t/h");
        let data = good();
        let pinned = "a".repeat(64);
        let dist = Dist {
            url: URL.into(),
            sha256: pinned.clone(),
        };
        let err = land(&Bytes::new(data.clone()), &dist, &dest, "libonnxruntime.a").unwrap_err();
        assert_eq!(
            err,
            format!(
                "{URL} has sha256 {}, not the pinned {pinned}",
                fetch::sha256_hex(&data)
            )
        );
        assert!(!s.path().join("cache").exists());
    }

    #[test]
    fn a_failed_fetch_removes_only_the_folders_it_made() {
        let s = scratch();
        let dest = s.path().join("cache/dfbin/t/h");
        let dist = dist_for(b"x");
        assert_eq!(
            land(&Fails, &dist, &dest, "libonnxruntime.a").unwrap_err(),
            format!("could not download {URL}: offline")
        );
        assert!(!s.path().join("cache").exists());
        fs::create_dir_all(s.path().join("cache/dfbin")).unwrap();
        assert!(land(&Fails, &dist, &dest, "libonnxruntime.a").is_err());
        assert!(s.path().join("cache/dfbin").is_dir());
        assert!(entries(&s.path().join("cache/dfbin")).is_empty());
    }

    #[test]
    fn a_stream_cut_before_its_end_marker_is_not_lzma2_and_leaves_nothing() {
        let s = scratch();
        let dest = s.path().join("cache/dfbin/t/h");
        let mut data = good();
        assert_eq!(
            data.pop(),
            Some(0),
            "raw LZMA2 ends with its 0x00 end marker"
        );
        for cut in [data.clone(), data[..data.len() / 2].to_vec()] {
            let err = land(
                &Bytes::new(cut.clone()),
                &dist_for(&cut),
                &dest,
                "libonnxruntime.a",
            )
            .unwrap_err();
            assert_eq!(err, format!("{URL} is not a raw LZMA2 stream"));
            assert!(!s.path().join("cache").exists());
        }
    }

    #[test]
    fn garbage_is_not_lzma2() {
        let s = scratch();
        let files = s.path();
        assert_eq!(
            unpack(&b"not lzma2 at all"[..], files, URL).unwrap_err(),
            format!("{URL} is not a raw LZMA2 stream")
        );
        assert!(entries(files).is_empty());
    }

    #[test]
    fn the_decoder_round_trips_the_encoder() {
        let s = scratch();
        let tar = fixture::tar(&["f:a", "f:b"]);
        unpack(&fixture::lzma2(&tar)[..], s.path(), URL).unwrap();
        assert_eq!(entries(s.path()), ["a", "b"]);
        let mut out = Vec::new();
        lzma_rust2::Lzma2Reader::new(&fixture::lzma2(&tar)[..], DICT_SIZE, None)
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, tar);
    }

    #[test]
    fn archive_refusals_name_the_url_and_leave_nothing() {
        for (tar, msg) in [
            (
                fixture::tar(&["f:lib/nested.a"]),
                "holds 'lib/nested.a', which is not a plain file at the top level",
            ),
            (
                fixture::tar(&["f:libwebgpu_dawn.so"]),
                "holds no libonnxruntime.a",
            ),
            (fixture::tar(&[]), "is an empty archive"),
            (fixture::tar(&["f:a", "f:a"]), "holds 'a' twice"),
            (
                fixture::tar(&["f:a"])[..600].to_vec(),
                "is not a readable archive: truncated entry",
            ),
        ] {
            let s = scratch();
            let dest = s.path().join("cache/dfbin/t/h");
            let data = fixture::lzma2(&tar);
            let err = land(
                &Bytes::new(data.clone()),
                &dist_for(&data),
                &dest,
                "libonnxruntime.a",
            )
            .unwrap_err();
            assert_eq!(err, format!("{URL} {msg}"));
            assert!(!s.path().join("cache").exists(), "{msg}");
        }
    }

    #[test]
    fn the_folder_another_process_landed_first_is_kept() {
        let s = scratch();
        let dest = s.path().join("cache/dfbin/t/h");
        let data = good();
        let mut source = Bytes::new(data.clone());
        let theirs = dest.clone();
        source.then = Box::new(move || {
            fs::create_dir_all(&theirs).unwrap();
            fs::write(theirs.join("libonnxruntime.a"), "theirs").unwrap();
        });
        land(&source, &dist_for(&data), &dest, "libonnxruntime.a").unwrap();
        assert_eq!(
            fs::read_to_string(dest.join("libonnxruntime.a")).unwrap(),
            "theirs"
        );
        assert_eq!(entries(&dest), ["libonnxruntime.a"]);
        assert_eq!(entries(dest.parent().unwrap()), ["h"]);
    }

    #[test]
    fn a_folder_another_process_landed_without_the_library_is_refused_and_kept() {
        let s = scratch();
        let dest = s.path().join("cache/dfbin/t/h");
        let data = good();
        let mut source = Bytes::new(data.clone());
        let theirs = dest.clone();
        source.then = Box::new(move || {
            fs::create_dir_all(&theirs).unwrap();
            fs::write(theirs.join("partial"), "theirs").unwrap();
        });
        let err = land(&source, &dist_for(&data), &dest, "libonnxruntime.a").unwrap_err();
        assert!(
            err.starts_with(&format!("cannot move the files into {}: ", dest.display())),
            "{err}"
        );
        assert_eq!(err.lines().count(), 1);
        assert_eq!(entries(&dest), ["partial"]);
        assert_eq!(entries(dest.parent().unwrap()), ["h"]);
    }

    #[test]
    fn the_hash_is_over_the_compressed_file() {
        let s = scratch();
        let data = good();
        let path = s.path().join("archive");
        fs::write(&path, &data).unwrap();
        let mut tar = Vec::new();
        lzma_rust2::Lzma2Reader::new(&data[..], DICT_SIZE, None)
            .read_to_end(&mut tar)
            .unwrap();
        assert_eq!(sha256_of(&path).unwrap(), fetch::sha256_hex(&data));
        assert_ne!(sha256_of(&path).unwrap(), fetch::sha256_hex(&tar));
        assert!(sha256_of(&s.path().join("missing")).is_err());
    }

    #[test]
    fn a_folder_that_cannot_be_written_names_the_file() {
        let s = scratch();
        let missing = s.path().join("missing");
        let err = unpack(&fixture::lzma2(&fixture::tar(&["f:a"]))[..], &missing, URL).unwrap_err();
        assert!(
            err.starts_with(&format!("cannot write {}: ", missing.join("a").display())),
            "{err}"
        );
    }

    fn serve(responses: Vec<Vec<u8>>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for response in responses {
                let Ok((mut conn, _)) = listener.accept() else {
                    return;
                };
                let mut request = Vec::new();
                let mut byte = [0u8; 1];
                while !request.ends_with(b"\r\n\r\n") && conn.read(&mut byte).unwrap_or(0) == 1 {
                    request.push(byte[0]);
                }
                if response.is_empty() {
                    let _ = conn.read(&mut byte);
                } else {
                    let _ = io::Write::write_all(&mut conn, &response);
                }
            }
        });
        format!("http://{addr}/ort.tar.lzma2")
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut r = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(body);
        r
    }

    fn quick() -> fetch::Timings {
        fetch::Timings {
            connect_timeout: std::time::Duration::from_secs(5),
            stall_timeout: std::time::Duration::from_millis(300),
            poll_interval: std::time::Duration::from_millis(20),
            backoff: [std::time::Duration::ZERO; 3],
        }
    }

    #[test]
    fn the_download_lands_the_pinned_bytes() {
        let s = scratch();
        let data = good();
        let url = serve(vec![ok(&data)]);
        let got = download(
            &fetch::Fetcher::new(s.path()),
            &url,
            &fetch::sha256_hex(&data),
        )
        .unwrap();
        assert_eq!(got, s.path().join("archive"));
        assert_eq!(fs::read(&got).unwrap(), data);
    }

    #[test]
    fn a_download_with_other_bytes_is_a_mismatch_and_keeps_nothing() {
        let s = scratch();
        let data = good();
        let url = serve(vec![ok(b"other bytes")]);
        let pinned = fetch::sha256_hex(&data);
        let err = download(&fetch::Fetcher::new(s.path()), &url, &pinned).unwrap_err();
        assert_eq!(
            err,
            format!(
                "{url} has sha256 {}, not the pinned {pinned}",
                fetch::sha256_hex(b"other bytes")
            )
        );
        assert!(entries(s.path()).is_empty());
    }

    #[test]
    fn a_refused_download_names_the_http_status_once() {
        let s = scratch();
        let url = serve(vec![
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        ]);
        let err = download(&fetch::Fetcher::new(s.path()), &url, &"a".repeat(64)).unwrap_err();
        assert_eq!(err, format!("could not download {url}: HTTP 404"));
    }

    #[test]
    fn an_unreachable_or_silent_server_is_named_plainly() {
        let s = scratch();
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/ort", closed.local_addr().unwrap());
        drop(closed);
        let fetcher = fetch::Fetcher::with_timings(s.path(), quick());
        assert_eq!(
            download(&fetcher, &url, &"a".repeat(64)).unwrap_err(),
            format!("could not reach {url}")
        );
        let url = serve(vec![Vec::new()]);
        assert_eq!(
            download(&fetcher, &url, &"a".repeat(64)).unwrap_err(),
            format!("{url} stalled")
        );
    }

    #[test]
    fn a_download_into_a_file_is_an_io_error() {
        let s = scratch();
        let file = s.path().join("file");
        fs::write(&file, b"").unwrap();
        let fetcher = fetch::Fetcher::with_timings(file.join("sub"), quick());
        let err = download(&fetcher, "http://127.0.0.1:9/ort", &"a".repeat(64)).unwrap_err();
        assert!(
            err.starts_with("could not download http://127.0.0.1:9/ort: io error at "),
            "{err}"
        );
    }

    #[test]
    fn the_network_source_takes_https_only() {
        let s = scratch();
        let err = Net
            .fetch("http://127.0.0.1:9/ort", &"a".repeat(64), s.path())
            .unwrap_err();
        assert_eq!(err, "http://127.0.0.1:9/ort is not an https:// URL");
        assert!(entries(s.path()).is_empty());
    }
}
