use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let resources_root = manifest_dir.join("resources/control-plane");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));

    println!("cargo:rerun-if-changed={}", resources_root.display());

    let mut assets = Vec::new();
    collect_files(&resources_root, Path::new(""), &mut assets);

    let mut generated = String::from("pub static EMBEDDED_ASSETS: &[(&str, &[u8])] = &[\n");
    for (logical, absolute) in assets {
        if logical.to_string_lossy().contains(".test.") {
            continue;
        }
        generated.push_str(&format!(
            "    ({:?}, include_bytes!(r#\"{}\"#)),\n",
            logical.to_string_lossy(),
            absolute.display(),
        ));
    }
    generated.push_str("];\n");
    fs::write(out_dir.join("embedded_assets.rs"), generated).expect("write embedded assets");

    tauri_build::build();
}

fn collect_files(root: &Path, logical_prefix: &Path, assets: &mut Vec<(PathBuf, PathBuf)>) {
    let entries = fs::read_dir(root).expect("read asset dir");
    for entry in entries {
        let entry = entry.expect("asset entry");
        let path = entry.path();
        let logical = logical_prefix.join(entry.file_name());
        if path.is_dir() {
            collect_files(&path, &logical, assets);
        } else {
            assets.push((logical, path));
        }
    }
}
