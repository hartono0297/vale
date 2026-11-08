//! What the transcript feature downloads: the transcribe.cpp native package
//! (pinned to one release, loaded at run time) and the speech models. Every
//! entry has a pinned URL, its exact size and its SHA-256.
//!
//! Sources (read 2026-11-07, nothing invented):
//! - The engine: the v0.4.1 release of github.com/handy-computer/transcribe.cpp,
//!   asset `transcribe-native-0.4.1-windows-x86_64-cpu-vulkan.tar.gz`. GitHub
//!   publishes the asset's SHA-256 (`digest`); the release has no `.sha256`
//!   file for it. `contract.json` inside names the header hash.
//! - The models: the Hugging Face repos named in the engine's own catalog
//!   (`catalog/<variant>.json`, `published_repo`). URLs are pinned to the repo
//!   commit, not `main`; the SHA-256 is the LFS object id Hugging Face lists.

/// One file to download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    /// HTTPS, pinned to a release tag or a repo commit.
    pub url: &'static str,
    /// The file name on disk (the last part of the URL).
    pub file: &'static str,
    /// Exact size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256 of the whole file.
    pub sha256: &'static str,
}

/// The native library package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Engine {
    pub version: &'static str,
    /// `header_hash` in the package's `contract.json` (and
    /// `include/transcribe.abihash` of the tag): the public C ABI the
    /// bindings in `engine.rs` were written against.
    pub header_hash: &'static str,
    pub asset: Asset,
    /// The library inside the package, loaded with `LoadLibraryExW`.
    pub dll: &'static str,
}

pub const ENGINE: Engine = Engine {
    version: "0.4.1",
    header_hash: "f99ad4169374b8f3",
    asset: Asset {
        url: "https://github.com/handy-computer/transcribe.cpp/releases/download/v0.4.1/transcribe-native-0.4.1-windows-x86_64-cpu-vulkan.tar.gz",
        file: "transcribe-native-0.4.1-windows-x86_64-cpu-vulkan.tar.gz",
        size: 18_052_295,
        sha256: "272eb97795048ccb557c85bb9dd44c9e530e1c928443114c39816a8a5f76269d",
    },
    dll: "transcribe.dll",
};

/// The model family: decides whether the model gives its own timestamps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Whisper: segment timestamps of its own, no length limit (it windows
    /// long audio itself).
    Whisper,
    /// Qwen3-ASR: text only, no timestamps, a hard input cap; Vale's own
    /// chunks carry the times.
    Qwen3Asr,
}

/// A speech-to-text model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// Stable id, the engine catalog's variant name (config and sidecar).
    pub key: &'static str,
    /// Shown in Settings and the transcript header.
    pub name: &'static str,
    pub quant: &'static str,
    pub family: Family,
    pub asset: Asset,
}

/// The two models Vale offers, in the Settings order: the GPU model first.
/// Measured on the owner's recordings: Whisper large-v3-turbo on a GPU,
/// Whisper small on the CPU only.
pub const MODELS: [Model; 2] = [
    Model {
        key: "whisper-large-v3-turbo",
        name: "Whisper large-v3-turbo",
        quant: "Q4_K_M",
        family: Family::Whisper,
        asset: Asset {
            url: "https://huggingface.co/handy-computer/whisper-large-v3-turbo-gguf/resolve/18ada752005f5fddef499256f9a458d0b37a203a/whisper-large-v3-turbo-Q4_K_M.gguf",
            file: "whisper-large-v3-turbo-Q4_K_M.gguf",
            size: 536_069_696,
            sha256: "c6e172d0074f0fa9440b88247854b50643cd4bb58b8515c797bbed16d858c7df",
        },
    },
    Model {
        key: "whisper-small",
        name: "Whisper small",
        quant: "Q4_K_M",
        family: Family::Whisper,
        asset: Asset {
            url: "https://huggingface.co/handy-computer/whisper-small-gguf/resolve/4afdc7942ecf1997f359deae48a63506b062eac4/whisper-small-Q4_K_M.gguf",
            file: "whisper-small-Q4_K_M.gguf",
            size: 171_630_656,
            sha256: "b204d2005a3e5d4fe6153bd61e5e8b32e757ff7b017ac8f61c6f051c2f80e939",
        },
    },
];

/// Silero VAD v6.2 for the engine's own voice detection (1.2 MB, MIT). It
/// comes with the engine; without it the chunker uses its energy detector.
pub const VAD: Asset = Asset {
    url: "https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/a9b2d61c20ae4ddf3644edb34969e8185a485448/silero-vad-v6.2-F32.gguf",
    file: "silero-vad-v6.2-F32.gguf",
    size: 1_241_056,
    sha256: "0d871a800c420fdd5d1b17a3aea4296d1c39608f0ff4a9295eb76457eb1c88e6",
};

/// The model a first download offers: turbo with a GPU, small without.
pub fn default_model(gpu: bool) -> &'static Model {
    if gpu { &MODELS[0] } else { &MODELS[1] }
}

/// Bytes as the nearest whole megabyte (Settings and the offer say "555 MB").
pub fn mb(bytes: u64) -> u64 {
    (bytes + 500_000) / 1_000_000
}

pub fn model(key: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.key == key)
}

/// `hex` as 32 bytes: exactly 64 hex digits, either case.
pub fn parse_sha256(hex: &str) -> Option<[u8; 32]> {
    let hex = hex.as_bytes();
    if hex.len() != 64 {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = [0u8; 32];
    for (i, pair) in hex.chunks_exact(2).enumerate() {
        out[i] = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_assets() -> Vec<Asset> {
        let mut all = vec![ENGINE.asset, VAD];
        all.extend(MODELS.iter().map(|m| m.asset));
        all
    }

    #[test]
    fn every_asset_is_pinned_and_hashed() {
        for asset in all_assets() {
            assert!(asset.url.starts_with("https://"), "{}", asset.url);
            assert!(asset.url.ends_with(&format!("/{}", asset.file)), "{}", asset.url);
            assert!(parse_sha256(asset.sha256).is_some(), "{}", asset.file);
            assert_eq!(asset.sha256, asset.sha256.to_ascii_lowercase());
            assert!(asset.size > 1_000_000, "{}", asset.file);
            let pinned = asset.url.contains("/releases/download/v")
                || asset.url.split('/').any(|part| {
                    part.len() == 40 && part.bytes().all(|b| b.is_ascii_hexdigit())
                });
            assert!(pinned, "not pinned to a tag or commit: {}", asset.url);
            assert!(!asset.url.contains("/resolve/main/"), "{}", asset.url);
        }
    }

    #[test]
    fn the_engine_package_is_the_pinned_release() {
        assert!(ENGINE.asset.url.contains(&format!("/v{}/", ENGINE.version)));
        assert!(ENGINE.asset.file.contains(ENGINE.version));
        assert!(ENGINE.asset.file.contains("windows-x86_64-cpu-vulkan"));
        assert_eq!(ENGINE.header_hash.len(), 16);
    }

    #[test]
    fn the_shipped_catalog_is_whisper_turbo_and_small() {
        assert_eq!(MODELS.len(), 2);
        assert_eq!(MODELS[0].key, "whisper-large-v3-turbo");
        assert_eq!(MODELS[1].key, "whisper-small");
        assert_eq!(model("qwen3-asr-0.6b"), None);
        assert_eq!(model("whisper-small").map(|m| m.family), Some(Family::Whisper));
        assert_ne!(MODELS[0].asset.file, MODELS[1].asset.file);
    }

    #[test]
    fn default_model_follows_the_gpu() {
        assert_eq!(default_model(true).key, "whisper-large-v3-turbo");
        assert_eq!(default_model(false).key, "whisper-small");
    }

    #[test]
    fn mb_rounds_to_the_nearest_megabyte() {
        let turbo = model("whisper-large-v3-turbo").unwrap().asset.size;
        let small = model("whisper-small").unwrap().asset.size;
        let runtime = ENGINE.asset.size + VAD.size;
        assert_eq!(mb(turbo), 536);
        assert_eq!(mb(small), 172);
        assert_eq!(mb(runtime), 19);
        assert_eq!(mb(turbo + runtime), 555);
        assert_eq!(mb(small + runtime), 191);
    }

    #[test]
    fn sha256_text_parses_strictly() {
        let bytes = parse_sha256(ENGINE.asset.sha256).unwrap();
        assert_eq!(bytes[0], 0x27);
        assert_eq!(bytes[31], 0x9d);
        assert!(parse_sha256(&ENGINE.asset.sha256.to_uppercase()).is_some());
        assert!(parse_sha256("abc").is_none());
        assert!(parse_sha256(&"g".repeat(64)).is_none());
    }
}
