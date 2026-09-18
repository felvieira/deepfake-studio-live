//! Os modelos ONNX.
//!
//! A lista e os tamanhos vêm de modules/model_downloader.py (MODEL_SIZES),
//! que também é quem baixa quando o app roda sem instalador. Baixar aqui é o
//! que evita o congelamento da primeira execução: hoje o app abre, o usuário
//! clica "Start live" e a janela trava por vários minutos sem feedback
//! nenhum, porque o download só mostra progresso no stdout.

/// Base do repositório no Hugging Face, igual a HF_RESOLVE_BASE do Python.
pub const HF_BASE: &str = "https://huggingface.co/hacksider/deep-live-cam/resolve/main/";

pub struct ModelFile {
    /// Caminho relativo dentro de models/, com '/' como separador.
    pub name: &'static str,
    pub size: u64,
}

/// O que o instalador baixa.
///
/// Note que `inswapper_128.onnx` (554 MB, fp32) **não** está aqui de
/// propósito: face_swapper.py chama ensure_any(["inswapper_128.onnx",
/// "inswapper_128_fp16.onnx"]), ou seja, qualquer uma das duas variantes
/// serve. O fp16 tem metade do tamanho, então baixar as duas gastaria 554 MB
/// à toa. Se algum dia o código passar a exigir o fp32, é aqui que se
/// acrescenta.
pub const MODELS: &[ModelFile] = &[
    ModelFile { name: "inswapper_128_fp16.onnx", size: 277_680_638 },
    ModelFile { name: "gfpgan-1024.onnx", size: 365_875_079 },
    ModelFile { name: "GPEN-BFR-256.onnx", size: 75_715_262 },
    ModelFile { name: "GPEN-BFR-512.onnx", size: 284_244_491 },
    ModelFile { name: "buffalo_l/buffalo_l/1k3d68.onnx", size: 143_607_619 },
    ModelFile { name: "buffalo_l/buffalo_l/2d106det.onnx", size: 5_030_888 },
    ModelFile { name: "buffalo_l/buffalo_l/det_10g.onnx", size: 16_923_827 },
    ModelFile { name: "buffalo_l/buffalo_l/genderage.onnx", size: 1_322_532 },
    ModelFile { name: "buffalo_l/buffalo_l/w600k_r50.onnx", size: 174_383_860 },
];

/// Soma dos tamanhos, para a UI mostrar o total antes de começar e para a
/// checagem de espaço em disco.
pub fn total_bytes() -> u64 {
    MODELS.iter().map(|m| m.size).sum()
}
