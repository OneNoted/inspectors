use std::path::PathBuf;

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("schemas"));
    desktop_core::write_schema_bundle(&out).expect("schema export should succeed");
    println!("exported schemas to {}", out.display());
}
