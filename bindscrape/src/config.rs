//! Configuration types for `bindscrape.toml`.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Root configuration.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub output: OutputConfig,
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
    pub fn wrapper_header(&self, base_dir: &Path) -> PathBuf {
        if self.headers.len() == 1 {
            base_dir.join(&self.headers[0])
        } else {
            // Generate a wrapper .c file that #includes all headers.
            let wrapper_dir = std::env::temp_dir().join("bindscrape_wrappers");
            std::fs::create_dir_all(&wrapper_dir).expect("create wrapper dir");

            // Use namespace as a stable filename
            let safe_name = self.namespace.replace('.', "_");
            let wrapper_path = wrapper_dir.join(format!("{safe_name}_wrapper.c"));

            let mut content = String::new();
            for h in &self.headers {
                let abs = if h.is_absolute() {
                    h.clone()
                } else {
                    base_dir.join(h)
                };
                content.push_str(&format!("#include \"{}\"\n", abs.display()));
            }
            std::fs::write(&wrapper_path, &content).expect("write wrapper file");
            wrapper_path
        }
    }
}

/// External winmd type imports (cross-winmd references).
#[derive(Debug, Deserialize)]
pub struct TypeImportConfig {
    /// Assembly name (e.g. `Windows.Win32`).
    pub assembly: String,
    /// Version string (e.g. `0.1.0.0`).
    #[serde(default)]
    pub version: Option<String>,
    /// Types to import from this assembly.
    pub types: Vec<ImportedType>,
}

/// A single imported type.
#[derive(Debug, Deserialize)]
pub struct ImportedType {
    pub name: String,
    pub namespace: String,
    #[serde(default)]
    pub interface: bool,
}

/// Load and parse a `bindscrape.toml` configuration file.
pub fn load_config(path: &Path) -> anyhow::Result<Config> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read config file {}: {}", path.display(), e))?;
    let config: Config = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse config file {}: {}", path.display(), e))?;
    Ok(config)
}
