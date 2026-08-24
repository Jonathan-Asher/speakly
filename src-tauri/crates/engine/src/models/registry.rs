//! Static catalog of downloadable models. Sizes are approximate (used for the
//! disk precheck and display); the download trusts Content-Length at runtime.

pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub size_bytes: u64,
    // TODO: pin sha256 digests and verify during download; until then the
    // downloader verifies byte count against Content-Length.
    pub sha256: Option<&'static str>,
    pub languages: &'static str,
    pub license: &'static str,
}

pub const REGISTRY: &[ModelInfo] = &[
    ModelInfo {
        id: "he-turbo",
        name: "Hebrew (ivrit.ai large-v3-turbo)",
        url: "https://huggingface.co/ivrit-ai/whisper-large-v3-turbo-ggml/resolve/main/ggml-model.bin",
        size_bytes: 1_620_000_000,
        sha256: None,
        languages: "Hebrew (fine-tuned), multilingual base",
        license: "Apache-2.0",
    },
    ModelInfo {
        id: "turbo",
        name: "English & multilingual (large-v3-turbo)",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        size_bytes: 1_624_000_000,
        sha256: None,
        languages: "99 languages",
        license: "MIT",
    },
    ModelInfo {
        id: "turbo-q8",
        name: "English & multilingual (turbo, q8 compact)",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q8_0.bin",
        size_bytes: 874_000_000,
        sha256: None,
        languages: "99 languages",
        license: "MIT",
    },
    ModelInfo {
        id: "turbo-q5",
        name: "English & multilingual (turbo, q5 small)",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        size_bytes: 574_000_000,
        sha256: None,
        languages: "99 languages",
        license: "MIT",
    },
    ModelInfo {
        id: "diar-seg",
        name: "Speaker detection (pyannote segmentation-3.0)",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx",
        size_bytes: 6_000_000,
        sha256: None,
        languages: "language-independent",
        license: "MIT",
    },
    ModelInfo {
        id: "diar-emb",
        name: "Speaker voices (3D-Speaker ERes2Net)",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx",
        size_bytes: 26_000_000,
        sha256: None,
        languages: "language-independent",
        license: "Apache-2.0",
    },
];

pub fn get(id: &str) -> Option<&'static ModelInfo> {
    REGISTRY.iter().find(|m| m.id == id)
}

/// Managed on-disk name inside the app's models directory. Whisper weights
/// are ggml `.bin`; diarization models are ONNX.
pub fn file_name(id: &str) -> String {
    if id.starts_with("diar-") {
        format!("{id}.onnx")
    } else {
        format!("ggml-{id}.bin")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lookup_and_names() {
        assert!(get("he-turbo").is_some());
        assert!(get("diar-seg").is_some());
        assert!(get("nope").is_none());
        assert_eq!(file_name("turbo-q8"), "ggml-turbo-q8.bin");
        assert_eq!(file_name("diar-emb"), "diar-emb.onnx");
    }
}
