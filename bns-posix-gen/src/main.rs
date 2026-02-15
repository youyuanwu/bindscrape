//! Generator that produces the `bns-posix` crate from POSIX system headers.
//!
//! This crate drives the **bindscrape → WinMD → windows-bindgen (package mode)**
//! pipeline. Run it to regenerate the `bns-posix` crate:
//!
//! ```sh
//! cargo run -p bns-posix-gen
//! ```

use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let bns_posix_dir = workspace_dir.join("bns-posix");

    bns_posix_gen::generate(&bns_posix_dir);

    println!("Generated bns-posix crate at {}", bns_posix_dir.display());
}
