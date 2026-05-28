use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const REPO: &str = "ZephyrCloudIO/zvec-rs";
const VERSION: &str = env!("CARGO_PKG_VERSION");

// DO NOT remove this rustfmt skip, the formatting is intentional so release CI can easily
// update the checksums for new releases by replacing the old ones in-place.
#[rustfmt::skip]
const CHECKSUMS: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-gnu", "6b420f7dd884be0677c10fdddb40c5181fae0ce7ac5458f67c31a02598101731"),
    ("aarch64-unknown-linux-gnu", "a14ec1342aa0de8ee8545dcd520f6f0b95968f910e155d34376197b89df9eb66"),
    ("x86_64-apple-darwin", "741a627ef288d572e49a345b47795bc328a8d7f2a7d9e9b4e7be03ad94ac0638"),
    ("aarch64-apple-darwin", "df0817b703004dc6e4f656f0289eb1823563820e329c229106fedfb58c45d755"),
    ("aarch64-apple-ios", ""),
    ("x86_64-pc-windows-msvc", "dc77218e28606e8a7f1eb2ca5ed32b767278e56219e1b017bbd6ce6d45103d87"),
    ("aarch64-linux-android", "729602537cb4dad3cb39f7e165cc2fd7a1682d25b00054af78becbd129799fa7"),
];

fn target_triple() -> String {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    match (arch.as_str(), os.as_str(), env.as_str()) {
        ("x86_64", "linux", "gnu") => "x86_64-unknown-linux-gnu".into(),
        ("aarch64", "linux", "gnu") => "aarch64-unknown-linux-gnu".into(),
        ("x86_64", "macos", _) => "x86_64-apple-darwin".into(),
        ("aarch64", "macos", _) => "aarch64-apple-darwin".into(),
        ("aarch64", "ios", _) => "aarch64-apple-ios".into(),
        ("x86_64", "windows", "msvc") => "x86_64-pc-windows-msvc".into(),
        ("aarch64", "android", _) => "aarch64-linux-android".into(),
        _ => panic!("unsupported target: {arch}-{os}-{env}"),
    }
}

fn lib_filename(os: &str) -> &'static str {
    match os {
        "windows" => "zvec_c_api.dll",
        "macos" | "ios" => "libzvec_c_api.dylib",
        _ => "libzvec_c_api.so",
    }
}

fn checksum_for_target(triple: &str) -> &'static str {
    CHECKSUMS
        .iter()
        .find(|(t, _)| *t == triple)
        .map(|(_, h)| *h)
        .unwrap_or_else(|| panic!("no checksum for target: {triple}"))
}

fn download_and_verify(url: &str, expected_sha256: &str, dest: &Path) {
    let resp = ureq::get(url)
        .call()
        .expect("failed to download vendor archive");
    let len: usize = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut body = Vec::with_capacity(len);
    resp.into_body()
        .as_reader()
        .read_to_end(&mut body)
        .expect("failed to read response body");

    assert!(
        !expected_sha256.is_empty(),
        "no checksum configured for {url} — run the release CI workflow"
    );
    let hash = hex::encode(Sha256::digest(&body));
    assert_eq!(
        hash, expected_sha256,
        "SHA256 mismatch for {url}: expected {expected_sha256}, got {hash}"
    );

    fs::write(dest, &body).expect("failed to write archive");
}

fn extract_tarball(archive_path: &Path, dest_dir: &Path) {
    let file = fs::File::open(archive_path).expect("failed to open archive");
    let gz = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    archive.unpack(dest_dir).expect("failed to extract archive");
}

fn copy_shared_libs_to_target(lib_dir: &Path) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target_dir = out_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();

    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let ext = match os.as_str() {
        "windows" => "dll",
        "macos" | "ios" => "dylib",
        _ => "so",
    };

    let examples_dir = target_dir.join("examples");
    let _ = fs::create_dir_all(&examples_dir);

    if let Ok(entries) = fs::read_dir(lib_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                let name = path.file_name().unwrap();
                let _ = fs::copy(&path, target_dir.join(name));
                let _ = fs::copy(&path, examples_dir.join(name));
            }
        }
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=ZVEC_LIB_DIR");

    let triple = target_triple();
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    let lib_dir = if let Ok(dir) = env::var("ZVEC_LIB_DIR") {
        PathBuf::from(dir)
    } else {
        let expected_sha256 = checksum_for_target(&triple);

        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let vendor_dir = out_dir.join("vendor");
        let lib_file = vendor_dir.join(lib_filename(&os));

        // Skip download if already extracted
        if !lib_file.exists() {
            let archive_name = format!("zvec_c_api-{triple}.tar.gz");
            let tag = format!("v{VERSION}");
            let url = format!("https://github.com/{REPO}/releases/download/{tag}/{archive_name}");

            let archive_path = out_dir.join(&archive_name);
            download_and_verify(&url, &expected_sha256, &archive_path);

            let _ = fs::create_dir_all(&vendor_dir);
            extract_tarball(&archive_path, &vendor_dir);
            let _ = fs::remove_file(&archive_path);
        }

        vendor_dir
    };

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=zvec_c_api");
    println!("cargo:lib_dir={}", lib_dir.display());

    copy_shared_libs_to_target(&lib_dir);

    let header = "include/zvec/c_api.h";
    println!("cargo:rerun-if-changed={header}");
    println!("cargo:rerun-if-changed=build.rs");

    let bindings = bindgen::Builder::default()
        .header(header)
        .allowlist_function("zvec_.*")
        .allowlist_type("ZVec.*")
        .allowlist_var("ZVEC_.*")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("failed to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("failed to write bindings");
}
