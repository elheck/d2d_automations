//! Embeds the built web UI (`check_stock/web/dist`, from `npm run build`)
//! into the server binary, so a deployment is one file.
//!
//! Without a build (the desktop release builds, CI jobs without Node) the
//! list is empty and the server shows a short hint page instead of the UI.
//! Cargo re-runs this script whenever anything under `web/dist` changes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let dist = manifest.join("..").join("web").join("dist");
    println!("cargo:rerun-if-changed={}", dist.display());

    let mut files = Vec::new();
    if dist.is_dir() {
        collect(&dist, &dist, &mut files);
    }
    files.sort();

    let mut code = String::from(
        "/// Files of the built web UI: path relative to `web/dist` (with `/`) and content.\n\
         pub static FILES: &[(&str, &[u8])] = &[\n",
    );
    for (relative, absolute) in &files {
        writeln!(code, "    ({relative:?}, include_bytes!({absolute:?})),")
            .expect("write to string");
    }
    code.push_str("];\n");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("set by cargo")).join("web_assets.rs");
    std::fs::write(out, code).expect("write web_assets.rs");
}

/// Every regular file below `dir`, as (path relative to `root`, absolute path).
fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    let entries = std::fs::read_dir(dir).expect("read web/dist");
    for entry in entries {
        let path = entry.expect("read web/dist entry").path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("below web/dist")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            // Already absolute (built from CARGO_MANIFEST_DIR); no canonicalize,
            // which would give `\\?\` paths on Windows.
            files.push((relative, path.to_string_lossy().into_owned()));
        }
    }
}
