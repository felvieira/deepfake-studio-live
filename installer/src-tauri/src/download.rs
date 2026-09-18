//! Download com retomada.
//!
//! São ~1 GB de modelos. Numa conexão doméstica isso quebra no meio com
//! alguma frequência, então cada arquivo baixa para `<nome>.part` e usa
//! Range para continuar de onde parou. O arquivo final só aparece quando o
//! conteúdo confere — assim uma instalação interrompida nunca deixa para trás
//! um .onnx truncado que o app tentaria carregar depois.

use futures_util::StreamExt;
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub struct DownloadSpec<'a> {
    pub url: &'a str,
    pub target: &'a Path,
    /// Tamanho esperado em bytes. Usado para validar o resultado e para a
    /// barra de progresso saber o total antes do servidor responder.
    pub expected_size: Option<u64>,
}

/// Baixa `spec.url` para `spec.target`, retomando um `.part` existente.
/// `on_progress` recebe (baixado, total) e é chamado a cada chunk.
pub async fn download_resumable<F>(
    client: &reqwest::Client,
    spec: &DownloadSpec<'_>,
    mut on_progress: F,
) -> Result<u64, String>
where
    F: FnMut(u64, u64),
{
    if let Some(parent) = spec.target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("não consegui criar {}: {e}", parent.display()))?;
    }

    // Já está lá e com o tamanho certo? Não baixa de novo — é o que torna
    // a reexecução depois de uma falha barata.
    if let Ok(meta) = tokio::fs::metadata(spec.target).await {
        if let Some(expected) = spec.expected_size {
            if meta.len() == expected {
                on_progress(expected, expected);
                return Ok(expected);
            }
        } else if meta.len() > 0 {
            on_progress(meta.len(), meta.len());
            return Ok(meta.len());
        }
    }

    let partial = spec.target.with_extension("part");
    let mut resume_from = tokio::fs::metadata(&partial)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    // Um .part maior que o esperado é lixo de um download anterior que deu
    // errado; recomeça em vez de mandar um Range inválido.
    if let Some(expected) = spec.expected_size {
        if resume_from >= expected {
            let _ = tokio::fs::remove_file(&partial).await;
            resume_from = 0;
        }
    }

    let mut request = client.get(spec.url);
    if resume_from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("falha de rede: {e}"))?;

    let status = response.status();
    // 416 = o servidor considera o range inválido. Recomeça do zero.
    if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err("faixa de bytes inválida; rode a instalação de novo".into());
    }
    if !status.is_success() {
        return Err(format!("servidor respondeu {status}"));
    }

    // Pediu retomada mas veio 200 em vez de 206: o servidor ignorou o Range
    // e está mandando o arquivo inteiro. Descarta o que tinha, senão o
    // resultado fica com bytes duplicados no começo.
    let honoured_range = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if resume_from > 0 && !honoured_range {
        resume_from = 0;
        let _ = tokio::fs::remove_file(&partial).await;
    }

    let total = spec
        .expected_size
        .or_else(|| response.content_length().map(|len| len + resume_from))
        .unwrap_or(0);

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(resume_from > 0)
        .write(true)
        .truncate(resume_from == 0)
        .open(&partial)
        .await
        .map_err(|e| format!("não consegui abrir {}: {e}", partial.display()))?;

    let mut downloaded = resume_from;
    let mut stream = response.bytes_stream();
    on_progress(downloaded, total);

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("conexão caiu: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("falha ao gravar: {e}"))?;
        downloaded += chunk.len() as u64;
        on_progress(downloaded, total.max(downloaded));
    }

    file.flush()
        .await
        .map_err(|e| format!("falha ao finalizar: {e}"))?;
    drop(file);

    if let Some(expected) = spec.expected_size {
        if downloaded != expected {
            // Não promove o .part: um arquivo do tamanho errado que vira
            // .onnx é pior que nenhum arquivo, porque o erro só aparece bem
            // depois, na hora de carregar o modelo.
            return Err(format!(
                "tamanho não confere: {downloaded} bytes, esperado {expected}. \
                 rode a instalação de novo para retomar"
            ));
        }
    }

    tokio::fs::rename(&partial, spec.target)
        .await
        .map_err(|e| format!("não consegui finalizar {}: {e}", spec.target.display()))?;

    Ok(downloaded)
}
