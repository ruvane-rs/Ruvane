use super::*;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

impl ClientPrerender {
    /// Writes a complete plan to a new regular file under trusted real directories.
    /// Preparation and all validation finish before file creation.
    pub fn write_new(&self, path: impl AsRef<Path>) -> Result<(), PrerenderError> {
        let bytes = self.encode();
        Self::decode(&bytes)?;
        let path = path.as_ref();
        real_parent(path)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(|_| PrerenderError::BuildIo)
    }
    /// Generates OUT_DIR/ruvane-prerender.rs for a trusted Cargo build script.
    /// RUVANE_PRERENDER_FILE is included only in native builds; Wasm uses an empty slice.
    pub fn write_build_module() -> Result<(), PrerenderError> {
        const MARKER: &str = "// ruvane-prerender-module 1\n";
        println!("cargo:rerun-if-env-changed=RUVANE_PRERENDER_FILE");
        let directory = PathBuf::from(std::env::var_os("OUT_DIR").ok_or(PrerenderError::BuildIo)?);
        let output = directory.join("ruvane-prerender.rs");
        real_parent(&output)?;
        let wasm = std::env::var("CARGO_CFG_TARGET_FAMILY")
            .unwrap_or_default()
            .split(',')
            .any(|value| value == "wasm");
        let contents = match std::env::var_os("RUVANE_PRERENDER_FILE").filter(|_| !wasm) {
            Some(path) => {
                let path = PathBuf::from(path);
                if !path.is_absolute() {
                    return Err(PrerenderError::BuildIo);
                }
                real_parent(&path)?;
                let metadata = fs::symlink_metadata(&path).map_err(|_| PrerenderError::BuildIo)?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() > MAX_CLIENT_PRERENDER_BYTES as u64
                {
                    return Err(PrerenderError::BuildIo);
                }
                let mut bytes = Vec::new();
                fs::File::open(&path)
                    .and_then(|file| {
                        file.take(MAX_CLIENT_PRERENDER_BYTES as u64 + 1)
                            .read_to_end(&mut bytes)
                    })
                    .map_err(|_| PrerenderError::BuildIo)?;
                Self::decode(&bytes)?;
                let text = path
                    .to_str()
                    .filter(|text| !text.chars().any(char::is_control))
                    .ok_or(PrerenderError::BuildIo)?;
                println!("cargo:rerun-if-changed={text}");
                format!("{MARKER}include_bytes!({text:?}) as &'static [u8]\n")
            }
            None => format!("{MARKER}&[] as &'static [u8]\n"),
        };
        if let Ok(metadata) = fs::symlink_metadata(&output) {
            if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 8192 {
                return Err(PrerenderError::BuildIo);
            }
            if !fs::read_to_string(&output)
                .map_err(|_| PrerenderError::BuildIo)?
                .starts_with(MARKER)
            {
                return Err(PrerenderError::BuildIo);
            }
        }
        let temp = directory.join(format!(".prerender-{}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| PrerenderError::BuildIo)?;
        if file
            .write_all(contents.as_bytes())
            .and_then(|_| fs::rename(&temp, &output))
            .is_err()
        {
            let _ = fs::remove_file(&temp);
            return Err(PrerenderError::BuildIo);
        }
        Ok(())
    }
}
fn real_parent(path: &Path) -> Result<(), PrerenderError> {
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(PrerenderError::BuildIo);
    }
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let absolute = if parent.is_absolute() {
        parent.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|_| PrerenderError::BuildIo)?
            .join(parent)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|_| PrerenderError::BuildIo)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(PrerenderError::BuildIo);
        }
    }
    Ok(())
}
