//! bnd-winmd — C header → WinMD metadata generator.
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
//! // Reads config TOML, parses headers, writes the .winmd file.
//! bnd_winmd::run(Path::new("bnd-winmd.toml"), None).unwrap();
//! ```
//!
//! Or get the raw bytes without writing to disk:
//!
//! ```no_run
//! use std::path::Path;
//!
//! let winmd_bytes = bnd_winmd::generate(Path::new("bnd-winmd.toml")).unwrap();
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
/// `config_path` is the path to a `bnd-winmd.toml` configuration file.  
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

/// Parse a `bnd-winmd.toml` config file, extract declarations from the
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
    let mut registry = extract::build_type_registry(&partitions, &cfg.namespace_overrides);

    // Pre-seed the registry with types from external winmd files
    // (cross-winmd references). This must happen after build_type_registry
    // so that locally-extracted types take priority (first-writer-wins in
    // the registry), but imported types fill in names that are referenced
    // by function signatures but not extracted locally.
    for ti in &cfg.type_import {
        let winmd_path = config::resolve_header(&ti.winmd, base_dir, &cfg.include_paths);
        seed_registry_from_winmd(&mut registry, &winmd_path, &ti.namespace);
    }

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

/// Pre-seed the [`TypeRegistry`](model::TypeRegistry) with types from an
/// external `.winmd` file.  Only types whose namespace starts with
/// `ns_filter` are imported.
fn seed_registry_from_winmd(
    registry: &mut model::TypeRegistry,
    winmd_path: &Path,
    ns_filter: &str,
) {
    let bytes = std::fs::read(winmd_path).unwrap_or_else(|e| {
        panic!(
            "failed to read external winmd {}: {e}\n\
             Hint: run the upstream gen crate first (e.g. `cargo run -p bnd-posix-gen`)",
            winmd_path.display()
        )
    });
    let file = windows_metadata::reader::File::new(bytes)
        .unwrap_or_else(|| panic!("failed to parse external winmd: {}", winmd_path.display()));
    let index = windows_metadata::reader::TypeIndex::new(vec![file]);
    let mut count = 0usize;
    for td in index.types() {
        let ns = td.namespace();
        let name = td.name();
        // Skip the synthetic <Module> and Apis classes, and filter by namespace.
        if ns.is_empty() || name == "<Module>" || name == "Apis" {
            continue;
        }
        if !ns.starts_with(ns_filter) {
            continue;
        }
        // Only insert if not already registered (local types win).
        if !registry.contains(name) {
            registry.register(name, ns);
            count += 1;
        }
    }
    info!(
        path = %winmd_path.display(),
        namespace = ns_filter,
        imported = count,
        "pre-seeded type registry from external winmd"
    );
}
