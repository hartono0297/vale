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

/// The four models of the design (section 5c), the default first.
pub const MODELS: [Model; 4] = [
    Model {
        key: "qwen3-asr-0.6b",
        name: "Qwen3-ASR 0.6B",
        quant: "Q4_K_M",
        family: Family::Qwen3Asr,
        asset: Asset {
            url: "https://huggingface.co/handy-computer/Qwen3-ASR-0.6B-gguf/resolve/7e7c69a577fa2725dfedbe34b5e999336e30a19b/Qwen3-ASR-0.6B-Q4_K_M.gguf",
            file: "Qwen3-ASR-0.6B-Q4_K_M.gguf",
            size: 589_560_480,
            sha256: "5b58f32a58ffa2c8783e0b0963485623e286e6272d953dfc9e28bc3447dee0c0",
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
    // Q8_0, the "~2.2 GB" of the approved design. The same repo commit has
    // Q4_K_M at 1,319,830,496 bytes, SHA-256
    // b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e.
    Model {
        key: "qwen3-asr-1.7b",
        name: "Qwen3-ASR 1.7B",
        quant: "Q8_0",
        family: Family::Qwen3Asr,
        asset: Asset {
            url: "https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf/resolve/0e0a596e3959c3f3f2ab7d443a5b6f0f4a4bba6f/Qwen3-ASR-1.7B-Q8_0.gguf",
            file: "Qwen3-ASR-1.7B-Q8_0.gguf",
            size: 2_185_030_624,
            sha256: "9a0d81792dfea2d5f278b8a63deb3ea6e02139ce42c2301f32ea19c4f77526b7",
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

/// The model a first download offers.
pub fn default_model() -> &'static Model {
    &MODELS[0]
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
    fn keys_are_unique_and_the_default_is_qwen3_small() {
        for (i, a) in MODELS.iter().enumerate() {
            for b in &MODELS[i + 1..] {
                assert_ne!(a.key, b.key);
                assert_ne!(a.asset.file, b.asset.file);
            }
        }
        assert_eq!(default_model().key, "qwen3-asr-0.6b");
        assert_eq!(model("whisper-small").map(|m| m.family), Some(Family::Whisper));
        assert_eq!(model("nope"), None);
    }

    #[test]
    fn sizes_match_the_design() {
        let mb = |key: &str| model(key).unwrap().asset.size / 1_000_000;
        assert_eq!(mb("qwen3-asr-0.6b"), 589);
        assert_eq!(mb("whisper-small"), 171);
        assert_eq!(mb("whisper-large-v3-turbo"), 536);
        assert_eq!(mb("qwen3-asr-1.7b"), 2185);
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
