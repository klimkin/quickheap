use std::path::{Path, PathBuf};

fn sources(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        files.push(path.to_owned());
    } else {
        for entry in std::fs::read_dir(path).unwrap() {
            sources(&entry.unwrap().path(), files);
        }
    }
}

fn main() {
    let mut files = vec![
        PathBuf::from("Cargo.toml"),
        PathBuf::from("Cargo.lock"),
        PathBuf::from("build.rs"),
        PathBuf::from("../src/fixture.rs"),
        PathBuf::from("../../../quickheap/Cargo.toml"),
        PathBuf::from("../../../.cargo/config.toml"),
    ];
    sources(Path::new("src"), &mut files);
    sources(Path::new("../../../quickheap/src"), &mut files);
    files.sort();
    let mut digest = 0xcbf29ce484222325u64;
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        for byte in path
            .to_string_lossy()
            .bytes()
            .chain(std::fs::read(&path).unwrap_or_default())
        {
            digest = (digest ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let version = std::process::Command::new(rustc)
        .arg("--version")
        .output()
        .unwrap();
    let version = String::from_utf8(version.stdout).unwrap();
    let flags = std::env::var("CARGO_ENCODED_RUSTFLAGS")
        .unwrap_or_default()
        .replace('\u{1f}', " ");
    let features = if std::env::var_os("CARGO_FEATURE_QUICKHEAP_AVX2").is_some() {
        "quickheap-avx2"
    } else {
        "standard-library"
    };
    let target = std::env::var("TARGET").unwrap();
    let opt = std::env::var("OPT_LEVEL").unwrap();
    println!(
        "cargo:rustc-env=PAIRED_BUILD={};target={target};opt={opt};features={features};flags={flags};source=fnv1a64:{digest:016x}",
        version.trim()
    );
}
