//! bindscrape — C header → WinMD metadata generator.
//!
//! Parses C headers via libclang and emits ECMA-335 `.winmd` files using the
//! `windows-metadata` writer crate.
//!
//! # Quick start
//!
//! Generate a `.winmd` file from a config (suitable for `build.rs`):
//!
//! ```no_run
//! use std::path::Path;
//!
//! // Reads bindscrape.toml, parses headers, writes the .winmd file.
//! bindscrape::run(Path::new("bindscrape.toml"), None).unwrap();
//! ```
//!
//! Or get the raw bytes without writing to disk:
//!
//! ```no_run
//! use std::path::Path;
//!
//! let winmd_bytes = bindscrape::generate(Path::new("bindscrape.toml")).unwrap();
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::info;

pub mod config;
pub mod emit;
pub mod extract;
pub mod model;

/// Run the full pipeline: load config, parse C headers, emit WinMD, and write
/// the output file.
///
/// `config_path` is the path to a `bindscrape.toml` file.  
/// `output` optionally overrides the output file path from the config.
///
/// This is the top-level entry point intended for use in `build.rs` scripts
/// or other programmatic callers that want the complete generate-and-write
/// workflow in a single call.
///
/// Returns the path the `.winmd` file was written to.
pub fn run(config_path: &Path, output: Option<&Path>) -> Result<PathBuf> {
    let cfg = config::load_config(config_path)
        .with_context(|| format!("loading config from {}", config_path.display()))?;

    let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

    let winmd_bytes = generate_from_config(&cfg, base_dir)?;

    let output_path = match output {
        Some(p) => p.to_path_buf(),
        None => base_dir.join(&cfg.output.file),
    };
    std::fs::write(&output_path, &winmd_bytes)
        .with_context(|| format!("writing output to {}", output_path.display()))?;

    info!(
        path = %output_path.display(),
        size = winmd_bytes.len(),
        "wrote winmd"
    );

    Ok(output_path)
}

/// Parse a `bindscrape.toml` config file, extract declarations from the
/// referenced C headers, and return the generated WinMD bytes without
/// writing to disk.
pub fn generate(config_path: &Path) -> Result<Vec<u8>> {
    let cfg = config::load_config(config_path)
        .with_context(|| format!("loading config from {}", config_path.display()))?;

    let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

    generate_from_config(&cfg, base_dir)
}

/// Generate WinMD bytes from an already-loaded [`config::Config`].
///
/// `base_dir` is the directory relative to which header paths in the config
/// are resolved (typically the parent directory of the TOML file).
pub fn generate_from_config(cfg: &config::Config, base_dir: &Path) -> Result<Vec<u8>> {
    info!(
        assembly = %cfg.output.name,
        partitions = cfg.partition.len(),
        "loaded configuration"
    );

    // Initialize clang
    let clang =
        clang::Clang::new().map_err(|e| anyhow::anyhow!("failed to initialize libclang: {e}"))?;
    let index = clang::Index::new(&clang, false, false);

    // Extract all partitions
    let mut partitions = Vec::new();
    for partition_cfg in &cfg.partition {
        let partition = extract::extract_partition(
            &index,
            partition_cfg,
            base_dir,
            &cfg.include_paths,
            &cfg.namespace_overrides,
        )?;
        partitions.push(partition);
    }

    // Build global type registry
    let registry = extract::build_type_registry(&partitions, &cfg.namespace_overrides);

    // Deduplicate typedefs: when the same typedef appears in multiple
    // partitions (e.g. `uid_t` in signal, stat, unistd, AND a shared types
    // partition), keep it only in the partition the registry maps it to.
    // The registry uses first-writer-wins for typedefs, so the types
    // partition should come first in the TOML to claim shared names.
    // Other partitions drop their local copy; any function/struct that
    // references the type will use a cross-partition TypeRef instead.
    for partition in &mut partitions {
        partition.typedefs.retain(|td| {
            let canonical_ns = registry.namespace_for(&td.name, &partition.namespace);
            canonical_ns == partition.namespace
        });
    }

    // Emit winmd
    let winmd_bytes = emit::emit_winmd(&cfg.output.name, &partitions, &registry)?;

    info!(size = winmd_bytes.len(), "generated winmd");

    Ok(winmd_bytes)
}
