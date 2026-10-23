use std::collections::BTreeSet;
use std::path::Path;

const ACCELERATORS: [&str; 21] = [
    "training", "tensorrt", "openvino", "onednn", "directml", "nnapi", "coreml", "xnnpack", "rocm",
    "acl", "armnn", "tvm", "migraphx", "rknpu", "vitis", "cann", "qnn", "webgpu", "azure", "nvrtx",
    "vsinpu",
];

#[derive(Debug, PartialEq, Eq)]
pub struct Dist {
    pub url: String,
    pub sha256: String,
}

pub fn wanted(enabled: &str) -> Result<BTreeSet<String>, String> {
    let on: BTreeSet<&str> = enabled
        .split([',', '\n'])
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect();
    if on.contains("cuda") || on.contains("tensorrt") {
        return Err("the cuda and tensorrt features of ort-sys are not supported".into());
    }
    Ok(on
        .into_iter()
        .filter(|f| ACCELERATORS.contains(f))
        .map(str::to_string)
        .collect())
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn resolve(
    table: &str,
    table_path: &Path,
    target: &str,
    want: &BTreeSet<String>,
) -> Result<Dist, String> {
    let rows: Vec<Vec<&str>> = table
        .split('\n')
        .skip(1)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|r| r[0] == target)
        .collect();
    if rows.is_empty() {
        return Err(format!("no ONNX Runtime build for {target}"));
    }
    for row in rows {
        let [_, features, url, sha256] = row[..] else {
            return Err(malformed(table_path, target));
        };
        if !url.starts_with("https://") || !is_sha256(sha256) {
            return Err(malformed(table_path, target));
        }
        let set: BTreeSet<String> = if features == "none" {
            BTreeSet::new()
        } else {
            features.split(',').map(str::to_string).collect()
        };
        if &set == want {
            return Ok(Dist {
                url: url.into(),
                sha256: sha256.into(),
            });
        }
    }
    let named: Vec<&str> = want.iter().map(String::as_str).collect();
    let named = if named.is_empty() {
        "none".to_string()
    } else {
        named.join(",")
    };
    Err(format!(
        "no ONNX Runtime build for {target} with features {named}"
    ))
}

fn malformed(table_path: &Path, target: &str) -> String {
    format!("{} has a malformed row for {target}", table_path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINUX: &str = "x86_64-unknown-linux-gnu";
    const MAC: &str = "aarch64-apple-darwin";

    fn table() -> String {
        let (zero, ones, twos) = ("0".repeat(64), "1".repeat(64), "2".repeat(64));
        format!(
            "target\tfeature_set\turl\tsha256_hash\n\
             # pinned builds\n\
             {LINUX}\tnone\thttps://example.invalid/none\t{zero}\n\
             {LINUX}\twebgpu\thttps://example.invalid/webgpu\t{ones}\n\
             \n\
             {MAC}\tcoreml,webgpu\thttps://example.invalid/mac\t{twos}\n"
        )
    }

    fn set(features: &[&str]) -> BTreeSet<String> {
        features.iter().map(|f| f.to_string()).collect()
    }

    fn pick(target: &str, features: &[&str]) -> Result<Dist, String> {
        resolve(&table(), Path::new("dist.tsv"), target, &set(features))
    }

    #[test]
    fn only_accelerator_features_count_and_order_does_not_matter() {
        assert_eq!(
            wanted("__tls,api-17,copy-dylibs,download-binaries,std,webgpu,coreml\n").unwrap(),
            set(&["coreml", "webgpu"])
        );
        assert_eq!(wanted("").unwrap(), set(&[]));
        assert_eq!(wanted("api-17,std").unwrap(), set(&[]));
    }

    #[test]
    fn cuda_and_tensorrt_are_refused() {
        for f in ["api-17,cuda", "tensorrt,webgpu", "cuda"] {
            assert_eq!(
                wanted(f).unwrap_err(),
                "the cuda and tensorrt features of ort-sys are not supported"
            );
        }
    }

    #[test]
    fn the_row_whose_feature_set_is_exactly_the_wanted_one_is_picked() {
        let d = pick(LINUX, &["webgpu"]).unwrap();
        assert_eq!(d.url, "https://example.invalid/webgpu");
        assert_eq!(d.sha256, "1".repeat(64));
        assert_eq!(pick(LINUX, &[]).unwrap().sha256, "0".repeat(64));
        assert_eq!(
            pick(MAC, &["webgpu", "coreml"]).unwrap().sha256,
            "2".repeat(64)
        );
    }

    #[test]
    fn a_subset_or_superset_is_no_match() {
        assert_eq!(
            pick(MAC, &["coreml"]).unwrap_err(),
            "no ONNX Runtime build for aarch64-apple-darwin with features coreml"
        );
        assert_eq!(
            pick(MAC, &[]).unwrap_err(),
            "no ONNX Runtime build for aarch64-apple-darwin with features none"
        );
        assert_eq!(
            pick(LINUX, &["coreml", "directml", "webgpu"]).unwrap_err(),
            "no ONNX Runtime build for x86_64-unknown-linux-gnu with features coreml,directml,webgpu"
        );
    }

    #[test]
    fn an_unknown_target_has_no_build() {
        assert_eq!(
            pick("sparc-sun-solaris", &[]).unwrap_err(),
            "no ONNX Runtime build for sparc-sun-solaris"
        );
    }

    #[test]
    fn the_header_line_is_never_a_row() {
        let t = format!("{LINUX}\tnone\thttps://x\t{}\n", "0".repeat(64));
        assert_eq!(
            resolve(&t, Path::new("t"), LINUX, &set(&[])).unwrap_err(),
            "no ONNX Runtime build for x86_64-unknown-linux-gnu"
        );
    }

    #[test]
    fn a_malformed_row_for_the_target_is_refused() {
        let good = "0".repeat(64);
        for row in [
            format!("{LINUX}\twebgpu\thttps://x"),
            format!("{LINUX}\twebgpu\thttps://x\t{good}\textra"),
            format!("{LINUX}\twebgpu\thttp://x\t{good}"),
            format!("{LINUX}\twebgpu\thttps://x\t{}", "A".repeat(64)),
            format!("{LINUX}\twebgpu\thttps://x\t{}", "0".repeat(63)),
        ] {
            let t = format!("header\n{row}\n");
            assert_eq!(
                resolve(&t, Path::new("/p/dist.tsv"), LINUX, &set(&["webgpu"])).unwrap_err(),
                format!("/p/dist.tsv has a malformed row for {LINUX}"),
                "{row}"
            );
        }
        let other = format!("header\n{MAC}\tbroken\n{LINUX}\tnone\thttps://x\t{good}\n");
        assert!(resolve(&other, Path::new("t"), LINUX, &set(&[])).is_ok());
    }
}
