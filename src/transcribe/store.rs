//! Where the engine and the models live on disk, under Vale's data folder:
//!
//! ```text
//! %APPDATA%\Vale\transcribe\
//!   engine\0.4.1\          transcribe.dll, ggml*.dll, contract.json, licenses\
//!   models\                <model>.gguf, silero-vad-v6.2-F32.gguf
//!   downloads\             <file>.part (resumable) and the engine tarball
//! ```
//!
//! A model counts as installed when its file has the catalog size: the
//! SHA-256 is checked once, before the download is renamed into place
//! (`download.rs`). The engine counts as installed when `transcribe.dll` is
//! there and `contract.json` names the pinned version and header hash.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::catalog::{self, Asset, ENGINE, Model};

/// The folder's name under the data folder.
pub const DIR: &str = "transcribe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    /// `%APPDATA%\Vale\transcribe`.
    pub fn default_location() -> Self {
        Store::new(crate::data_dir::dir().join(DIR))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The unpacked native package of the pinned version.
    pub fn engine_dir(&self) -> PathBuf {
        self.root.join("engine").join(ENGINE.version)
    }

    /// Where the package is unpacked before it is renamed to `engine_dir`, so
    /// a half-unpacked engine is never the engine.
    pub fn engine_staging_dir(&self) -> PathBuf {
        self.root
            .join("engine")
            .join(format!("{}.staging", ENGINE.version))
    }

    pub fn models_dir(&self) -> PathBuf {
        self.root.join("models")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    pub fn model_path(&self, model: &Model) -> PathBuf {
        self.models_dir().join(model.asset.file)
    }

    pub fn vad_path(&self) -> PathBuf {
        self.models_dir().join(catalog::VAD.file)
    }

    /// The queue of recordings waiting for a transcript.
    pub fn queue_path(&self) -> PathBuf {
        self.root.join("queue.json")
    }

    /// The partial download of `asset`, kept between runs to resume.
    pub fn part_path(&self, asset: &Asset) -> PathBuf {
        self.downloads_dir().join(format!("{}.part", asset.file))
    }

    pub fn engine_installed(&self) -> bool {
        let dir = self.engine_dir();
        dir.join(ENGINE.dll).is_file()
            && fs::read_to_string(dir.join("contract.json"))
                .is_ok_and(|text| check_contract(&text).is_ok())
    }

    pub fn model_installed(&self, model: &Model) -> bool {
        has_size(&self.model_path(model), model.asset.size)
    }

    pub fn vad_installed(&self) -> bool {
        has_size(&self.vad_path(), catalog::VAD.size)
    }

    pub fn installed_models(&self) -> Vec<&'static Model> {
        catalog::MODELS
            .iter()
            .filter(|m| self.model_installed(m))
            .collect()
    }

    /// Removes the model and its partial download. Recordings and
    /// transcripts live elsewhere and are not touched.
    pub fn delete_model(&self, model: &Model) -> io::Result<()> {
        remove_file_if_there(&self.part_path(&model.asset))?;
        remove_file_if_there(&self.model_path(model))
    }

    /// Removes the partial downloads of `assets` (after a failed checksum or
    /// a cancelled setup the user will not resume).
    pub fn discard_parts(&self, assets: &[Asset]) -> io::Result<()> {
        for asset in assets {
            remove_file_if_there(&self.part_path(asset))?;
        }
        Ok(())
    }

    /// Removes the engine, the VAD model and their partial downloads. Fails
    /// while the engine is loaded in this process (Windows keeps a loaded
    /// DLL's file).
    pub fn delete_engine(&self) -> io::Result<()> {
        remove_file_if_there(&self.part_path(&ENGINE.asset))?;
        remove_file_if_there(&self.part_path(&catalog::VAD))?;
        remove_file_if_there(&self.vad_path())?;
        remove_dir_if_there(&self.engine_staging_dir())?;
        remove_dir_if_there(&self.engine_dir())
    }

    /// Removes what no current catalog entry owns: engines of other
    /// versions, a leftover staging folder, partial downloads and `.gguf`
    /// models of files no longer offered. Installed models of the catalog stay.
    pub fn sweep(&self) -> io::Result<()> {
        let keep_engine = self.engine_dir();
        if let Ok(entries) = fs::read_dir(self.root.join("engine")) {
            for entry in entries.flatten() {
                if entry.path() != keep_engine {
                    remove_any(&entry.path())?;
                }
            }
        }
        if let Ok(entries) = fs::read_dir(self.models_dir()) {
            let owned: Vec<&str> = catalog::MODELS
                .iter()
                .map(|m| m.asset.file)
                .chain([catalog::VAD.file])
                .collect();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".gguf") && !owned.contains(&name.as_str()) {
                    remove_any(&entry.path())?;
                }
            }
        }
        let wanted: Vec<String> = catalog::MODELS
            .iter()
            .map(|m| m.asset)
            .chain([ENGINE.asset, catalog::VAD])
            .map(|a| format!("{}.part", a.file))
            .collect();
        if let Ok(entries) = fs::read_dir(self.downloads_dir()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !wanted.contains(&name) {
                    remove_any(&entry.path())?;
                }
            }
        }
        Ok(())
    }

    /// Bytes on disk under the store (Settings: "Terpakai 610 MB").
    pub fn used_bytes(&self) -> u64 {
        dir_size(&self.root)
    }
}

/// Checks the package's `contract.json` against the pinned version and
/// header hash.
pub fn check_contract(text: &str) -> Result<(), String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("contract.json: {e}"))?;
    let field = |key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or("");
    if field("version") != ENGINE.version {
        return Err(format!(
            "engine version {:?}, expected {}",
            field("version"),
            ENGINE.version
        ));
    }
    if field("header_hash") != ENGINE.header_hash {
        return Err(format!(
            "engine header hash {:?}, expected {}",
            field("header_hash"),
            ENGINE.header_hash
        ));
    }
    Ok(())
}

fn has_size(path: &Path, size: u64) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == size)
}

fn remove_file_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn remove_dir_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn remove_any(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        remove_dir_if_there(path)
    } else {
        remove_file_if_there(path)
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => dir_size(&entry.path()),
            Ok(_) => entry.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh store under `target/test-tmp/`, never `%APPDATA%`.
    pub(crate) fn temp_store(name: &str) -> Store {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("transcribe-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("test dir");
        Store::new(dir)
    }

    pub(crate) const CONTRACT: &str = r#"{
  "version": "0.4.1",
  "header_hash": "f99ad4169374b8f3",
  "backends": ["vulkan", "cpu"],
  "lane": "cpu-vulkan"
}"#;

    fn sized_file(path: &Path, size: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = fs::File::create(path).unwrap();
        file.set_len(size).unwrap();
    }

    #[test]
    fn the_layout_sits_under_the_transcribe_folder() {
        let store = Store::new(r"C:\data\Vale\transcribe");
        assert_eq!(
            store.engine_dir(),
            PathBuf::from(r"C:\data\Vale\transcribe\engine\0.4.1")
        );
        let turbo = catalog::default_model(true);
        assert_eq!(
            store.model_path(turbo),
            PathBuf::from(r"C:\data\Vale\transcribe\models\whisper-large-v3-turbo-Q4_K_M.gguf")
        );
        assert_eq!(
            store.part_path(&turbo.asset),
            PathBuf::from(
                r"C:\data\Vale\transcribe\downloads\whisper-large-v3-turbo-Q4_K_M.gguf.part"
            )
        );
    }

    #[test]
    fn a_model_is_installed_only_at_its_catalog_size_and_deletes_cleanly() {
        let store = temp_store("models");
        let small = catalog::model("whisper-small").unwrap();
        assert!(!store.model_installed(small));
        sized_file(&store.model_path(small), small.asset.size - 1);
        assert!(!store.model_installed(small), "a short file is not installed");
        sized_file(&store.model_path(small), small.asset.size);
        sized_file(&store.part_path(&small.asset), 10);
        assert!(store.model_installed(small));
        assert_eq!(store.installed_models(), vec![small]);
        assert!(store.used_bytes() >= small.asset.size);

        store.delete_model(small).unwrap();
        assert!(!store.model_installed(small));
        assert!(!store.part_path(&small.asset).exists());
        store.delete_model(small).expect("deleting twice is fine");
    }

    #[test]
    fn the_engine_needs_its_dll_and_a_matching_contract() {
        let store = temp_store("engine");
        let dir = store.engine_dir();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("contract.json"), CONTRACT).unwrap();
        assert!(!store.engine_installed(), "no dll yet");
        fs::write(dir.join(ENGINE.dll), b"MZ").unwrap();
        assert!(store.engine_installed());

        fs::write(dir.join("contract.json"), CONTRACT.replace("0.4.1", "0.5.0")).unwrap();
        assert!(!store.engine_installed());

        sized_file(&store.vad_path(), catalog::VAD.size);
        assert!(store.vad_installed());
        store.delete_engine().unwrap();
        assert!(!dir.exists());
        assert!(!store.vad_installed());
    }

    #[test]
    fn the_contract_check_names_what_differs() {
        assert_eq!(check_contract(CONTRACT), Ok(()));
        let other_hash = CONTRACT.replace("f99ad4169374b8f3", "0000000000000000");
        assert!(check_contract(&other_hash).unwrap_err().contains("header hash"));
        assert!(check_contract("{}").unwrap_err().contains("version"));
        assert!(check_contract("not json").is_err());
    }

    #[test]
    fn sweep_keeps_the_current_engine_and_known_downloads() {
        let store = temp_store("sweep");
        fs::create_dir_all(store.engine_dir()).unwrap();
        fs::create_dir_all(store.root().join("engine").join("0.3.1")).unwrap();
        fs::create_dir_all(store.engine_staging_dir()).unwrap();
        let qwen = catalog::default_model(true);
        sized_file(&store.part_path(&qwen.asset), 5);
        sized_file(&store.downloads_dir().join("old-model.gguf.part"), 5);
        sized_file(&store.model_path(qwen), qwen.asset.size);

        store.sweep().unwrap();
        assert!(store.engine_dir().is_dir());
        assert!(!store.root().join("engine").join("0.3.1").exists());
        assert!(!store.engine_staging_dir().exists());
        assert!(store.part_path(&qwen.asset).exists());
        assert!(!store.downloads_dir().join("old-model.gguf.part").exists());
        assert!(store.model_installed(qwen));
    }

    #[test]
    fn sweep_removes_models_no_longer_offered() {
        let store = temp_store("sweep-models");
        let turbo = catalog::default_model(true);
        let old = store.models_dir().join("Qwen3-ASR-0.6B-Q4_K_M.gguf");
        sized_file(&old, 7);
        sized_file(&store.model_path(turbo), turbo.asset.size);
        sized_file(&store.vad_path(), catalog::VAD.size);

        store.sweep().unwrap();
        assert!(!old.exists());
        assert!(store.model_installed(turbo));
        assert!(store.vad_installed());
    }

    #[test]
    fn discard_parts_removes_only_those_parts() {
        let store = temp_store("discard");
        let turbo = catalog::default_model(true).asset;
        let small = catalog::default_model(false).asset;
        sized_file(&store.part_path(&turbo), 5);
        sized_file(&store.part_path(&catalog::VAD), 5);
        sized_file(&store.part_path(&small), 5);

        store.discard_parts(&[turbo, catalog::VAD]).unwrap();
        assert!(!store.part_path(&turbo).exists());
        assert!(!store.part_path(&catalog::VAD).exists());
        assert!(store.part_path(&small).exists());
        store.discard_parts(&[turbo]).expect("a missing part is fine");
    }

    #[test]
    fn queue_path_is_beside_the_models() {
        let store = Store::new(r"C:\data\Vale\transcribe");
        assert_eq!(store.queue_path(), PathBuf::from(r"C:\data\Vale\transcribe\queue.json"));
    }
}
