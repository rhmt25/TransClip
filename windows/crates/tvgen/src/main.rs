//! Writes the shared protocol test vectors to `docs/test-vectors/`.
//!
//! Usage: `cargo run -p tvgen [output-dir]`. Running it twice must not change any file.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir: PathBuf = match std::env::args_os().nth(1) {
        Some(p) => p.into(),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../docs/test-vectors"),
    };
    std::fs::create_dir_all(&dir)?;
    for (name, value) in proto::vectors::generate()? {
        let mut text = serde_json::to_string_pretty(&value)?;
        text.push('\n');
        std::fs::write(dir.join(name), text)?;
        println!("wrote {name}");
    }
    Ok(())
}
