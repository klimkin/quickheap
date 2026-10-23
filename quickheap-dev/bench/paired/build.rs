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

fn profile_setting(profile: &str, key: &str, default: &str) -> String {
    let variable = format!(
        "CARGO_PROFILE_{}_{}",
        profile.to_uppercase(),
        key.replace('-', "_").to_uppercase()
    );
    println!("cargo:rerun-if-env-changed={variable}");
    if let Ok(value) = std::env::var(&variable) {
        return value;
    }
    let result = std::process::Command::new(std::env::var("CARGO").unwrap())
        .args([
            "-Z",
            "unstable-options",
            "config",
            "get",
            &format!("profile.{profile}.{key}"),
        ])
        .output()
        .unwrap();
    if result.status.success() {
        let text = String::from_utf8(result.stdout).unwrap();
        return text
            .split_once('=')
            .unwrap()
            .1
            .trim()
            .trim_matches('"')
            .to_owned();
    }
    if String::from_utf8_lossy(&result.stderr).contains("is not set") {
        return default.to_owned();
    }
    "unresolved".to_owned()
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
    let profile = std::env::var("PROFILE").unwrap();
    let cargo_profile = if profile == "debug" { "dev" } else { "release" };
    let lto = profile_setting(cargo_profile, "lto", "false");
    let codegen = profile_setting(
        cargo_profile,
        "codegen-units",
        if profile == "debug" { "256" } else { "16" },
    );
    println!(
        "cargo:rustc-env=PAIRED_BUILD={};target={target};opt={opt};lto={lto};codegen-units={codegen};features={features};flags={flags};source=fnv1a64:{digest:016x}",
        version.trim()
    );
}
