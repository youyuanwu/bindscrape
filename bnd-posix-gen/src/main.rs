//! Generator that produces the `bnd-posix` crate from POSIX system headers.
//!
//! This crate drives the **bnd-winmd → WinMD → windows-bindgen (package mode)**
//! pipeline. Run it to regenerate the `bnd-posix` crate:
//!
//! ```sh
//! cargo run -p bnd-posix-gen
//! ```

use std::path::PathBuf;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let workspace_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let bnd_posix_dir = workspace_dir.join("bnd-posix");

    bnd_posix_gen::generate(&bnd_posix_dir);

    println!("Generated bnd-posix crate at {}", bnd_posix_dir.display());
}
