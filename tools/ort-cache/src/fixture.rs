use std::io::Write;

pub fn reseal(header: &mut [u8]) {
    header[148..156].copy_from_slice(b"        ");
    let sum: u64 = header[..512].iter().map(|b| *b as u64).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
}

pub fn entry(name: &str, kind: u8, data: &[u8], link: &str) -> Vec<u8> {
    let mut h = vec![0u8; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[100..108].copy_from_slice(b"0000644\0");
    h[108..116].copy_from_slice(b"0000000\0");
    h[116..124].copy_from_slice(b"0000000\0");
    h[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
    h[136..148].copy_from_slice(b"00000000000\0");
    h[156] = kind;
    h[157..157 + link.len()].copy_from_slice(link.as_bytes());
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    reseal(&mut h);
    h.extend_from_slice(data);
    h.resize(h.len().next_multiple_of(512), 0);
    h
}

pub fn tar_of(entries: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = entries.concat();
    out.extend_from_slice(&[0; 1024]);
    out
}

pub fn tar(specs: &[&str]) -> Vec<u8> {
    let entries: Vec<Vec<u8>> = specs
        .iter()
        .map(|spec| {
            let (kind, name) = spec.split_once(':').expect("a spec is kind:name");
            match kind {
                "d" => entry(&format!("{name}/"), b'5', b"", ""),
                "l" => entry(name, b'2', b"", "/etc/passwd"),
                "h" => entry(name, b'1', b"", "other"),
                "x" => entry(name, b'x', b"", ""),
                "L" => entry(name, b'L', b"", ""),
                _ => entry(name, b'0', name.as_bytes(), ""),
            }
        })
        .collect();
    tar_of(&entries)
}

pub fn lzma2(data: &[u8]) -> Vec<u8> {
    let mut options = lzma_rust2::Lzma2Options::with_preset(6);
    options.lzma_options.dict_size = 1 << 16;
    let mut writer = lzma_rust2::Lzma2Writer::new(Vec::new(), options);
    writer.write_all(data).expect("the encoder takes the bytes");
    writer.finish().expect("the encoder finishes")
}
