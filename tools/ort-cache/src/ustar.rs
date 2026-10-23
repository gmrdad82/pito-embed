use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;

const BLOCK: usize = 512;

#[derive(Debug)]
pub enum TarError {
    NotPlain(String),
    Twice(String),
    Corrupt(&'static str),
    Read(io::Error),
    Write(String, io::Error),
}

struct Header([u8; BLOCK]);

fn parse_octal(bytes: &[u8]) -> Option<u64> {
    let s: String = bytes
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as char)
        .collect();
    u64::from_str_radix(s.trim(), 8).ok()
}

fn parse_str(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).unwrap_or("")
}

fn fill<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize, TarError> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(TarError::Read(e)),
        }
    }
    Ok(got)
}

impl Header {
    fn read_from<R: Read>(r: &mut R) -> Result<Option<Self>, TarError> {
        let mut data = [0; BLOCK];
        match fill(r, &mut data)? {
            0 => Ok(None),
            BLOCK if data.iter().all(|b| *b == 0) => Ok(None),
            BLOCK => Ok(Some(Self(data))),
            _ => Err(TarError::Corrupt("truncated header")),
        }
    }

    fn slice(&self, offset: usize, len: usize) -> &[u8] {
        &self.0[offset..offset + len]
    }

    fn checksum(&self) -> u64 {
        self.0
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    *b as u64
                }
            })
            .sum()
    }

    fn kind(&self) -> u8 {
        self.0[156]
    }

    fn path(&self) -> String {
        let prefix = parse_str(self.slice(345, 155));
        let name = parse_str(self.slice(0, 100));
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if self.kind() == b'5' {
            path.trim_end_matches('/').to_string()
        } else {
            path
        }
    }
}

fn top_level_name(path: &str) -> Option<&str> {
    let name = path.strip_prefix("./").unwrap_or(path);
    let plain = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\');
    plain.then_some(name)
}

pub fn extract<R: Read>(reader: &mut R, out: &Path) -> Result<usize, TarError> {
    let mut pad = [0; BLOCK];
    let mut count = 0;
    while let Some(header) = Header::read_from(reader)? {
        let sum =
            parse_octal(header.slice(148, 8)).ok_or(TarError::Corrupt("bad checksum field"))?;
        if sum != header.checksum() {
            return Err(TarError::Corrupt("tar entry checksum does not match"));
        }
        let path = header.path();
        let name = match (header.kind(), top_level_name(&path)) {
            (b'0' | 0, Some(name)) => name,
            _ => return Err(TarError::NotPlain(path)),
        };
        let size = parse_octal(header.slice(124, 12)).ok_or(TarError::Corrupt("bad size field"))?;
        let target = out.join(name);
        let file = File::create_new(&target).map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => TarError::Twice(path.clone()),
            _ => TarError::Write(target.display().to_string(), e),
        })?;
        let mut file = BufWriter::new(file);
        let mut body = reader.by_ref().take(size);
        let mut buf = vec![0; 1 << 16];
        let mut copied = 0u64;
        loop {
            let n = fill(&mut body, &mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| TarError::Write(target.display().to_string(), e))?;
            copied += n as u64;
        }
        file.flush()
            .map_err(|e| TarError::Write(target.display().to_string(), e))?;
        if copied != size {
            return Err(TarError::Corrupt("truncated entry"));
        }
        let padding = (size.next_multiple_of(BLOCK as u64) - size) as usize;
        if fill(reader, &mut pad[..padding])? != padding {
            return Err(TarError::Corrupt("truncated entry"));
        }
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture;

    fn scratch() -> tempfile::TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tmp/testscratch");
        std::fs::create_dir_all(&root).unwrap();
        tempfile::Builder::new()
            .prefix("ustar-")
            .tempdir_in(root)
            .unwrap()
    }

    fn run(tar: &[u8]) -> (Result<usize, TarError>, tempfile::TempDir) {
        let dir = scratch();
        let got = extract(&mut &tar[..], dir.path());
        (got, dir)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn a_flat_archive_extracts_every_file_with_its_bytes() {
        let big = vec![7u8; 1500];
        let tar = fixture::tar_of(&[
            fixture::entry("libonnxruntime.a", b'0', b"lib", ""),
            fixture::entry("big.bin", b'0', &big, ""),
            fixture::entry("empty", b'0', b"", ""),
        ]);
        let (got, dir) = run(&tar);
        assert_eq!(got.unwrap(), 3);
        assert_eq!(names(dir.path()), ["big.bin", "empty", "libonnxruntime.a"]);
        assert_eq!(
            std::fs::read(dir.path().join("libonnxruntime.a")).unwrap(),
            b"lib"
        );
        assert_eq!(std::fs::read(dir.path().join("big.bin")).unwrap(), big);
        assert!(std::fs::read(dir.path().join("empty")).unwrap().is_empty());
    }

    #[test]
    fn one_leading_dot_slash_is_accepted() {
        let (got, dir) = run(&fixture::tar(&["f:./libonnxruntime.a"]));
        assert_eq!(got.unwrap(), 1);
        assert_eq!(names(dir.path()), ["libonnxruntime.a"]);
    }

    #[test]
    fn an_old_style_nul_type_flag_is_a_plain_file() {
        let (got, dir) = run(&fixture::tar_of(&[fixture::entry("a", 0, b"x", "")]));
        assert_eq!(got.unwrap(), 1);
        assert_eq!(names(dir.path()), ["a"]);
    }

    #[test]
    fn anything_but_a_top_level_plain_file_is_refused_and_written_nowhere() {
        for (spec, name) in [
            ("f:lib/nested.a", "lib/nested.a"),
            ("f:./lib/nested.a", "./lib/nested.a"),
            ("f:../escape.a", "../escape.a"),
            ("f:./../escape.a", "./../escape.a"),
            ("f:././a", "././a"),
            ("f:/etc/escape.a", "/etc/escape.a"),
            ("f:a\\b", "a\\b"),
            ("f:.", "."),
            ("f:./", "./"),
            ("d:lib", "lib"),
            ("l:libonnxruntime.a", "libonnxruntime.a"),
            ("h:libonnxruntime.a", "libonnxruntime.a"),
            ("x:PaxHeaders/a", "PaxHeaders/a"),
            ("x:pax", "pax"),
            ("L:././@LongLink", "././@LongLink"),
        ] {
            let (got, dir) = run(&fixture::tar(&[spec, "f:after"]));
            match got {
                Err(TarError::NotPlain(n)) => assert_eq!(n, name, "{spec}"),
                other => panic!("{spec}: {other:?}"),
            }
            assert!(names(dir.path()).is_empty(), "{spec}");
            assert!(!dir.path().parent().unwrap().join("escape.a").exists());
        }
    }

    #[test]
    fn a_long_name_through_the_ustar_prefix_is_still_nested() {
        let mut e = fixture::entry("a", b'0', b"x", "");
        e[345..348].copy_from_slice(b"dir");
        fixture::reseal(&mut e);
        match run(&fixture::tar_of(&[e])).0 {
            Err(TarError::NotPlain(n)) => assert_eq!(n, "dir/a"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_file_named_twice_is_refused_without_overwriting() {
        let tar = fixture::tar_of(&[
            fixture::entry("a", b'0', b"first", ""),
            fixture::entry("./a", b'0', b"second", ""),
        ]);
        let (got, dir) = run(&tar);
        assert!(matches!(got, Err(TarError::Twice(n)) if n == "./a"));
        assert_eq!(std::fs::read(dir.path().join("a")).unwrap(), b"first");
    }

    #[test]
    fn a_bad_checksum_is_refused_before_anything_is_written() {
        let mut e = fixture::entry("a", b'0', b"x", "");
        e[0] = b'b';
        let (got, dir) = run(&fixture::tar_of(&[e]));
        assert!(matches!(
            got,
            Err(TarError::Corrupt("tar entry checksum does not match"))
        ));
        assert!(names(dir.path()).is_empty());
    }

    #[test]
    fn a_non_octal_size_or_checksum_is_corrupt() {
        let mut e = fixture::entry("a", b'0', b"x", "");
        e[124..136].copy_from_slice(b"zzzzzzzzzzz\0");
        fixture::reseal(&mut e);
        assert!(matches!(
            run(&fixture::tar_of(&[e])).0,
            Err(TarError::Corrupt("bad size field"))
        ));
        let mut e = fixture::entry("a", b'0', b"x", "");
        e[148..156].copy_from_slice(b"zzzzzz\0 ");
        assert!(matches!(
            run(&fixture::tar_of(&[e])).0,
            Err(TarError::Corrupt("bad checksum field"))
        ));
    }

    #[test]
    fn a_truncated_entry_header_or_padding_is_corrupt() {
        let tar = fixture::tar(&["f:libonnxruntime.a"]);
        for (cut, why) in [
            (BLOCK + 4, "truncated entry"),
            (BLOCK + 100, "truncated entry"),
            (300, "truncated header"),
        ] {
            let (got, _dir) = run(&tar[..cut]);
            match got {
                Err(TarError::Corrupt(w)) => assert_eq!(w, why, "{cut}"),
                other => panic!("{cut}: {other:?}"),
            }
        }
    }

    #[test]
    fn an_empty_archive_or_no_bytes_hold_no_files() {
        assert_eq!(run(&fixture::tar(&[])).0.unwrap(), 0);
        assert_eq!(run(&[]).0.unwrap(), 0);
        let (got, _dir) = run(&fixture::tar(&["f:a"])[..BLOCK * 2]);
        assert_eq!(got.unwrap(), 1);
    }

    #[test]
    fn a_read_error_surfaces_as_a_read_error() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("boom"))
            }
        }
        let dir = scratch();
        assert!(matches!(
            extract(&mut Broken, dir.path()),
            Err(TarError::Read(_))
        ));
    }

    #[test]
    fn an_interrupted_read_is_retried() {
        struct Once<'a> {
            data: &'a [u8],
            interrupted: bool,
        }
        impl Read for Once<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                self.data.read(buf)
            }
        }
        let dir = scratch();
        let tar = fixture::tar(&["f:a"]);
        let mut reader = Once {
            data: &tar,
            interrupted: false,
        };
        assert_eq!(extract(&mut reader, dir.path()).unwrap(), 1);
        assert!(reader.interrupted);
        assert_eq!(std::fs::read(dir.path().join("a")).unwrap(), b"a");
    }

    #[test]
    fn a_folder_that_cannot_be_written_is_a_write_error() {
        let dir = scratch();
        let missing = dir.path().join("missing");
        let tar = fixture::tar(&["f:a"]);
        assert!(
            matches!(extract(&mut &tar[..], &missing), Err(TarError::Write(p, _)) if p.ends_with("missing/a"))
        );
    }
}
