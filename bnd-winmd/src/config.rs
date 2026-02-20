//! Configuration types for `bnd-winmd.toml`.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Root configuration.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub output: OutputConfig,
    /// Additional directories to search when resolving header and traverse
    /// paths.  Each entry is tried in order after `base_dir` (the TOML
    /// file's parent directory).  Also injected as `-I` flags for clang.
    #[serde(default)]
    pub include_paths: Vec<PathBuf>,
    #[serde(default)]
    pub partition: Vec<PartitionConfig>,
    #[serde(default)]
    pub namespace_overrides: HashMap<String, String>,
    #[serde(default)]
    pub type_import: Vec<TypeImportConfig>,
}

/// Output file settings.
#[derive(Debug, Deserialize)]
pub struct OutputConfig {
    /// Assembly name written into the winmd.
    pub name: String,
    /// Output file path (e.g. `MyLib.winmd`).
    #[serde(default = "default_output_file")]
    pub file: PathBuf,
}

fn default_output_file() -> PathBuf {
    PathBuf::from("output.winmd")
}

/// A single partition — maps a set of headers to one namespace.
#[derive(Debug, Deserialize)]
pub struct PartitionConfig {
    /// ECMA-335 namespace (e.g. `MyLib.Graphics`).
    pub namespace: String,
    /// Library name for P/Invoke `ImplMap` entries (e.g. `mylib.so`).
    pub library: String,
    /// Headers to include (all are parsed for dependency resolution).
    pub headers: Vec<PathBuf>,
    /// Which files to actually emit declarations from.
    /// If empty, uses `headers`.
    #[serde(default)]
    pub traverse: Vec<PathBuf>,
    /// Extra clang arguments (e.g. `-I/usr/include`).
    #[serde(default)]
    pub clang_args: Vec<String>,
}

impl PartitionConfig {
    /// Returns the traverse list, falling back to `headers` if empty.
    pub fn traverse_files(&self) -> &[PathBuf] {
        if self.traverse.is_empty() {
            &self.headers
        } else {
            &self.traverse
        }
    }

    /// Returns the translation unit file to parse.
    ///
    /// If there's a single header/source file, returns it directly.
    /// If there are multiple, generates a wrapper `.c` file in `out_dir`
    /// that `#include`s all of them — mimicking the scraper `.c` files
    /// that win32metadata uses.
    pub fn wrapper_header(&self, base_dir: &Path, include_paths: &[PathBuf]) -> PathBuf {
        if self.headers.len() == 1 {
            resolve_header(&self.headers[0], base_dir, include_paths)
        } else {
            // Generate a wrapper .c file that #includes all headers.
            let wrapper_dir = std::env::temp_dir().join("bnd_winmd_wrappers");
            std::fs::create_dir_all(&wrapper_dir).expect("create wrapper dir");

            // Use namespace as a stable filename
            let safe_name = self.namespace.replace('.', "_");
            let wrapper_path = wrapper_dir.join(format!("{safe_name}_wrapper.c"));

            let mut content = String::new();
            for h in &self.headers {
                let abs = resolve_header(h, base_dir, include_paths);
                content.push_str(&format!("#include \"{}\"\n", abs.display()));
            }
            std::fs::write(&wrapper_path, &content).expect("write wrapper file");
            wrapper_path
        }
    }
}

/// Resolve a header path by searching `base_dir` first, then each
/// `include_paths` entry.  Absolute paths are returned as-is.  If the
/// file is not found anywhere, falls back to `base_dir.join(path)` so
/// that the caller gets a meaningful error from clang.
pub fn resolve_header(path: &Path, base_dir: &Path, include_paths: &[PathBuf]) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let candidate = base_dir.join(path);
    if candidate.exists() {
        return candidate;
    }
    for inc in include_paths {
        let candidate = inc.join(path);
        if candidate.exists() {
            return candidate;
        }
    }
    // Fall back — clang will report the error with context.
    base_dir.join(path)
}

/// External winmd type imports (cross-winmd references).
///
/// Pre-seeds the `TypeRegistry` with types from an external winmd so that
/// `ctype_to_wintype()` emits TypeRef rows instead of falling back to the
/// resolved canonical type.
///
/// ```toml
/// [[type_import]]
/// winmd = "../bnd-posix/winmd/bnd-posix.winmd"
/// namespace = "posix"
/// ```
#[derive(Debug, Deserialize)]
pub struct TypeImportConfig {
    /// Path to the external `.winmd` file (resolved relative to the TOML
    /// file's directory, i.e. `base_dir`).
    pub winmd: PathBuf,
    /// Root namespace filter — only types under this namespace tree are
    /// imported into the registry.
    pub namespace: String,
}

/// Load and parse a `bnd-winmd.toml` configuration file.
pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read config file {}: {}", path.display(), e))?;
    let config: Config = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse config file {}: {}", path.display(), e))?;
    Ok(config)
}
