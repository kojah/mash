use std::fmt::Write as _;
use std::process::Command;

fn main() {
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
    embed_dashboard();
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

/// Embeds the built `mash dash` front end (`dashboard/dist`) when it exists;
/// without it the command serves a page saying how to build it, unless
/// `MASH_REQUIRE_DASHBOARD` makes its absence a build error.
fn embed_dashboard() {
    // Read at run time. `env!` would bake in the checkout that compiled this
    // script, and a target directory shared between checkouts reuses that
    // binary: the build would then embed another checkout's front end.
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let dist = std::path::Path::new(&manifest).join("../dashboard/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    // A release must ship the dashboard, so its build sets this and fails here
    // instead of quietly producing a binary without it.
    println!("cargo:rerun-if-env-changed=MASH_REQUIRE_DASHBOARD");
    let required =
        std::env::var_os("MASH_REQUIRE_DASHBOARD").is_some_and(|value| !value.is_empty());
    assert!(
        !required || dist.join("index.html").is_file(),
        "MASH_REQUIRE_DASHBOARD is set but {} has no index.html; run `pnpm install --frozen-lockfile && pnpm build` in dashboard/ first",
        dist.display()
    );
    let mut files = Vec::new();
    collect(&dist, &dist, &mut files);
    files.sort();
    let mut source = String::from("pub(super) static ASSETS: &[(&str, &[u8])] = &[\n");
    for (route, path) in files {
        let _ = writeln!(
            source,
            "    ({route:?}, include_bytes!({:?})),",
            path.display().to_string()
        );
    }
    source.push_str("];\n");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("dash_assets.rs"), source).expect("write dashboard assets");
}

fn collect(
    root: &std::path::Path,
    dir: &std::path::Path,
    files: &mut Vec<(String, std::path::PathBuf)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            collect(root, &path, files);
        } else if let Ok(relative) = path.strip_prefix(root) {
            let route = format!("/{}", relative.to_string_lossy().replace('\\', "/"));
            files.push((route, path));
        }
    }
}
