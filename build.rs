use std::process::Command;

fn main() {
    #[cfg(feature = "vec1")]
    build_vec1();
    let override_revision = std::env::var("MASH_BUILD_REVISION").ok();
    let revision = override_revision.clone().unwrap_or_else(|| {
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .unwrap_or_else(|| "unknown".into())
    });
    println!("cargo:rerun-if-env-changed=MASH_BUILD_REVISION");
    let revision = revision.trim();
    if revision != "unknown"
        && (revision.len() != 40
            || !revision
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    {
        eprintln!("MASH_BUILD_REVISION must be a full lowercase Git commit ID");
        std::process::exit(1);
    }
    println!("cargo:rustc-env=MASH_BUILD_REVISION={}", revision.trim());
    println!("cargo:rerun-if-changed=build.rs");
    if override_revision.is_none() {
        for name in ["HEAD", "refs/heads", "packed-refs"] {
            if let Ok(output) = Command::new("git")
                .args(["rev-parse", "--git-path", name])
                .output()
                && output.status.success()
                && let Ok(path) = String::from_utf8(output.stdout)
                && std::path::Path::new(path.trim()).exists()
            {
                println!("cargo:rerun-if-changed={}", path.trim());
            }
        }
    }
}

#[cfg(feature = "vec1")]
fn build_vec1() {
    for file in ["app/storage/vec1/vec1.c", "app/storage/vec1/register.c"] {
        println!("cargo:rerun-if-changed={file}");
    }
    cc::Build::new()
        .files(["app/storage/vec1/vec1.c", "app/storage/vec1/register.c"])
        .include(std::env::var("DEP_SQLITE3_INCLUDE").expect("bundled SQLite headers"))
        .define("VEC1_STATIC", None)
        .define("NDEBUG", None)
        .warnings(false)
        .opt_level(2)
        // Retain the native initializer even though application Rust has no FFI.
        .link_lib_modifier("+whole-archive")
        .compile("mash_vec1");
    if std::env::var("CARGO_CFG_TARGET_FAMILY").as_deref() == Ok("unix") {
        println!("cargo:rustc-link-lib=m");
    }
}
