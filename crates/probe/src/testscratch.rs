pub(crate) fn dir(label: &str) -> tempfile::TempDir {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp/testscratch");
    std::fs::create_dir_all(&root).expect("test scratch root is creatable");
    tempfile::Builder::new()
        .prefix(label)
        .tempdir_in(root)
        .expect("test scratch dir is creatable")
}
