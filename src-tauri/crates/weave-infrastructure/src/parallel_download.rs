//! Sdílená logika stahování souborů pro `model_manager` a `comfy_installer`.
//!
//! Jedno TCP/TLS spojení bývá na CDN (HuggingFace, CivitAI...) throughput-limitované
//! výrazně pod reálnou šířku pásma linky — nástroje jako LM Studio nebo Free Download
//! Manager proto stahují velké soubory přes víc paralelních spojení (HTTP Range).
//! Když server podporu Range hlásí (`Accept-Ranges: bytes` + známý `Content-Length`),
//! rozdělíme soubor na segmenty a stáhneme je souběžně; jinak padneme zpět na jeden
//! sekvenční stream (chová se přesně jako dřív).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::header::{ACCEPT_RANGES, RANGE};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

/// Pod touto velikostí souboru (nebo segmentu) se paralelizace nevyplatí —
/// režie navíc spojení by převážila nad ziskem.
const MIN_PARALLEL_SIZE: u64 = 32 * 1024 * 1024;
const MIN_SEGMENT_SIZE: u64 = 16 * 1024 * 1024;
const MAX_SEGMENTS: u64 = 8;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

/// Stáhne `url` do `dest`. `on_progress(downloaded, total)` se volá throttlovaně
/// (max. ~5x/s) — je to synchronní callback (typicky `tx.try_send(...)` do
/// existujícího progress kanálu), aby ho šlo volat i z interního tickeru.
pub async fn download(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<u64, String> {
    let probe = http
        .head(url)
        .send()
        .await
        .map_err(|e| format!("HEAD požadavek selhal: {e}"))?;

    // Response::content_length() se u HEAD odpovědi odvozuje od skutečného
    // (prázdného) těla, ne od hlavičky — vrátí vždy 0 bez ohledu na to, co
    // hlásí Content-Length. Proto ji čteme napřímo z hlaviček.
    let total = probe
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let supports_range = probe
        .headers()
        .get(ACCEPT_RANGES)
        .map(|v| v.as_bytes() == b"bytes")
        .unwrap_or(false);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    if !supports_range || total < MIN_PARALLEL_SIZE {
        return download_sequential(http, url, dest, total, on_progress).await;
    }

    download_segmented(http, url, dest, total, on_progress).await
}

async fn download_sequential(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    total_hint: u64,
    on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<u64, String> {
    use std::io::{BufWriter, Write};

    let response = http
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Stažení selhalo: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Server vrátil {}", response.status()));
    }

    let total = response.content_length().unwrap_or(total_hint);
    let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut file = BufWriter::with_capacity(256 * 1024, file);

    let mut downloaded = 0u64;
    let mut last_report = tokio::time::Instant::now();
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("Stažení selhalo: {e}"))?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        downloaded += bytes.len() as u64;

        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = tokio::time::Instant::now();
            on_progress(downloaded, total);
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    on_progress(downloaded, total);
    Ok(downloaded)
}

async fn download_segmented(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    total: u64,
    on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<u64, String> {
    // Soubor předalokujeme na plnou velikost, ať do něj segmenty můžou psát
    // na svůj offset nezávisle na sobě (řídké/preallocated zápisy, ne append).
    {
        let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
        file.set_len(total).map_err(|e| e.to_string())?;
    }

    let num_segments = (total / MIN_SEGMENT_SIZE).clamp(1, MAX_SEGMENTS);
    let segment_size = total.div_ceil(num_segments);

    let downloaded = Arc::new(AtomicU64::new(0));
    let on_progress = Arc::new(on_progress);

    let ticker_downloaded = downloaded.clone();
    let ticker_progress = on_progress.clone();
    let ticker = tokio::spawn(async move {
        loop {
            tokio::time::sleep(PROGRESS_INTERVAL).await;
            let d = ticker_downloaded.load(Ordering::Relaxed);
            ticker_progress(d, total);
            if d >= total {
                break;
            }
        }
    });

    let mut tasks = Vec::new();
    for i in 0..num_segments {
        let start = i * segment_size;
        if start >= total {
            break;
        }
        let end = (start + segment_size).min(total) - 1;
        let http = http.clone();
        let url = url.to_string();
        let dest = dest.to_path_buf();
        let downloaded = downloaded.clone();
        tasks.push(tokio::spawn(async move {
            download_range(&http, &url, &dest, start, end, &downloaded).await
        }));
    }

    for task in tasks {
        task.await.map_err(|e| e.to_string())??;
    }

    ticker.abort();
    on_progress(total, total);
    Ok(total)
}

async fn download_range(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    start: u64,
    end: u64,
    downloaded: &AtomicU64,
) -> Result<(), String> {
    let response = http
        .get(url)
        .header(RANGE, format!("bytes={start}-{end}"))
        .send()
        .await
        .map_err(|e| format!("Segment {start}-{end} selhal: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Segment {start}-{end}: server vrátil {}",
            response.status()
        ));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(dest)
        .await
        .map_err(|e| e.to_string())?;
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|e| e.to_string())?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("Segment {start}-{end} selhal: {e}"))?;
        file.write_all(&bytes).await.map_err(|e| e.to_string())?;
        downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed);
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn tmp_dest(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("weave_parallel_dl_{name}_{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn falls_back_to_sequential_without_range_support() {
        let server = MockServer::start().await;
        let body = b"hello world".to_vec();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;
        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200).insert_header("content-length", body.len().to_string()),
            )
            .mount(&server)
            .await;

        let dest = tmp_dest("sequential");
        let progress = Arc::new(Mutex::new(Vec::new()));
        let progress_clone = progress.clone();

        let downloaded = download(&reqwest::Client::new(), &server.uri(), &dest, move |d, t| {
            progress_clone.lock().unwrap().push((d, t));
        })
        .await
        .unwrap();

        assert_eq!(downloaded, body.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        let _ = std::fs::remove_file(&dest);
    }

    #[tokio::test]
    async fn downloads_in_segments_when_range_supported() {
        let server = MockServer::start().await;
        let total_size = 40 * 1024 * 1024u64; // > MIN_PARALLEL_SIZE
        let full_body: Vec<u8> = (0..total_size).map(|i| (i % 256) as u8).collect();

        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", total_size.to_string())
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;

        // Vrátí odpovídající slice podle Range hlavičky.
        let body_for_mock = full_body.clone();
        Mock::given(method("GET"))
            .respond_with(move |req: &wiremock::Request| {
                let full_body = &body_for_mock;
                let range = req
                    .headers
                    .get("range")
                    .expect("segmentovany download musi poslat Range")
                    .to_str()
                    .unwrap();
                let spec = range.strip_prefix("bytes=").unwrap();
                let (start, end) = spec.split_once('-').unwrap();
                let start: usize = start.parse().unwrap();
                let end: usize = end.parse().unwrap();
                ResponseTemplate::new(206).set_body_bytes(full_body[start..=end].to_vec())
            })
            .mount(&server)
            .await;

        let dest = tmp_dest("segmented");
        let downloaded = download(&reqwest::Client::new(), &server.uri(), &dest, |_, _| {})
            .await
            .unwrap();

        assert_eq!(downloaded, total_size);
        assert_eq!(std::fs::read(&dest).unwrap(), full_body);
        let _ = std::fs::remove_file(&dest);
    }
}
