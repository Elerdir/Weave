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
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, RANGE};
use reqwest::StatusCode;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

/// Pod touto velikostí souboru (nebo segmentu) se paralelizace nevyplatí —
/// režie navíc spojení by převážila nad ziskem.
const MIN_PARALLEL_SIZE: u64 = 32 * 1024 * 1024;
const MIN_SEGMENT_SIZE: u64 = 16 * 1024 * 1024;
const DEFAULT_MAX_SEGMENTS: u64 = 16;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

static MAX_SEGMENTS_OVERRIDE: AtomicU64 = AtomicU64::new(0);

pub fn set_max_segments_override(segments: u64) {
    MAX_SEGMENTS_OVERRIDE.store(segments.clamp(1, 32), Ordering::Relaxed);
}

pub fn current_max_segments() -> u64 {
    max_segments()
}

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
    let mut total = probe
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut supports_range = probe
        .headers()
        .get(ACCEPT_RANGES)
        .map(|v| v.as_bytes() == b"bytes")
        .unwrap_or(false);

    if !supports_range && (total == 0 || total >= MIN_PARALLEL_SIZE) {
        if let Ok(range_probe) = http.get(url).header(RANGE, "bytes=0-0").send().await {
            if range_probe.status() == StatusCode::PARTIAL_CONTENT {
                supports_range = true;
                if total == 0 {
                    total = range_probe
                        .headers()
                        .get(CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok())
                        .and_then(parse_content_range_total)
                        .unwrap_or(0);
                }
            }
        }
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let result = if !supports_range || total < MIN_PARALLEL_SIZE {
        download_sequential(http, url, dest, total, on_progress).await
    } else {
        download_segmented(http, url, dest, total, on_progress).await
    };

    // Po selhání rozpracovaný soubor smažeme — segmentovaný režim ho
    // předalokuje na plnou velikost, takže by se soubor s dírami tvářil
    // jako kompletní (a např. llama.cpp by na něm spadl). Resume neumíme,
    // částečný soubor nemá hodnotu.
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
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
    if response.error_for_status_ref().is_err() {
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

    let num_segments = (total / MIN_SEGMENT_SIZE).clamp(1, max_segments());
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

    // Výsledky sbíráme ze VŠECH segmentů bez předčasného návratu — ticker
    // se musí ukončit vždy. (Dřív se při chybě segmentu `?` vrátil rovnou,
    // JoinHandle tickeru se jen dropnul a jeho smyčka běžela navěky, protože
    // `downloaded` už nikdy nedosáhlo `total`.)
    let mut result: Result<(), String> = Ok(());
    for task in tasks {
        let outcome = match task.await {
            Ok(r) => r,
            Err(join_err) => Err(join_err.to_string()),
        };
        if let (Ok(()), Err(e)) = (&result, outcome) {
            result = Err(e);
        }
    }
    ticker.abort();

    result?;
    on_progress(total, total);
    Ok(total)
}

/// Kolikrát zkusit segment znovu — velké soubory (10+ GB) běží desítky minut
/// a jeden přechodný síťový výpadek by jinak zahodil celé stahování.
const SEGMENT_ATTEMPTS: u32 = 3;

async fn download_range(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    start: u64,
    end: u64,
    downloaded: &AtomicU64,
) -> Result<(), String> {
    let mut last_err = String::new();
    for attempt in 1..=SEGMENT_ATTEMPTS {
        match download_range_once(http, url, dest, start, end, downloaded).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::warn!(
                    segment = format!("{start}-{end}"),
                    attempt,
                    error = %e,
                    "Segment stahování selhal"
                );
                last_err = e;
                if attempt < SEGMENT_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(500 * u64::from(attempt))).await;
                }
            }
        }
    }
    Err(format!("po {SEGMENT_ATTEMPTS} pokusech: {last_err}"))
}

/// Jeden pokus o segment. Při chybě vrátí čítač `downloaded` o bajty tohoto
/// pokusu zpět, aby je opakování nezapočítalo dvakrát (progress by přeskočil
/// přes 100 %). Zápis jde na pevný offset, takže opakování je bezpečné.
async fn download_range_once(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    start: u64,
    end: u64,
    downloaded: &AtomicU64,
) -> Result<(), String> {
    let mut written = 0u64;
    let result = async {
        let response = http
            .get(url)
            .header(RANGE, format!("bytes={start}-{end}"))
            .send()
            .await
            .map_err(|e| format!("Segment {start}-{end} selhal: {e}"))?;

        if response.status() != StatusCode::PARTIAL_CONTENT {
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
            written += bytes.len() as u64;
            downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed);
        }
        file.flush().await.map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;

    if result.is_err() && written > 0 {
        downloaded.fetch_sub(written, Ordering::Relaxed);
    }
    result
}

fn max_segments() -> u64 {
    let override_value = MAX_SEGMENTS_OVERRIDE.load(Ordering::Relaxed);
    if override_value > 0 {
        return override_value;
    }
    std::env::var("WEAVE_DOWNLOAD_SEGMENTS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|v| v.clamp(1, 32))
        .unwrap_or(DEFAULT_MAX_SEGMENTS)
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    let (_, total) = value.split_once('/')?;
    if total == "*" {
        return None;
    }
    total.parse().ok()
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

        let downloaded = download(
            &reqwest::Client::new(),
            &server.uri(),
            &dest,
            move |d, t| {
                progress_clone.lock().unwrap().push((d, t));
            },
        )
        .await
        .unwrap();

        assert_eq!(downloaded, body.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        let _ = std::fs::remove_file(&dest);
    }

    #[tokio::test]
    async fn failed_segmented_download_removes_preallocated_file() {
        let server = MockServer::start().await;
        let total_size = 40 * 1024 * 1024u64;

        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", total_size.to_string())
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;
        // Všechny segmenty trvale selžou (i po retry)
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let dest = tmp_dest("failed_cleanup");
        let result = download(&reqwest::Client::new(), &server.uri(), &dest, |_, _| {}).await;

        assert!(result.is_err());
        // Předalokovaný soubor s dírami NESMÍ zůstat na disku — tvářil by se
        // jako kompletní model.
        assert!(!dest.exists(), "po chybě nesmí zůstat rozpracovaný soubor");
    }

    #[tokio::test]
    async fn segment_retry_recovers_from_transient_error() {
        let server = MockServer::start().await;
        let total_size = 40 * 1024 * 1024u64;
        let full_body: Vec<u8> = (0..total_size).map(|i| (i % 256) as u8).collect();

        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", total_size.to_string())
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;

        // První GET selže (přechodný výpadek), další už vrací správné slices.
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let body_for_mock = full_body.clone();
        Mock::given(method("GET"))
            .respond_with(move |req: &wiremock::Request| {
                let range = req.headers.get("range").unwrap().to_str().unwrap();
                let spec = range.strip_prefix("bytes=").unwrap();
                let (start, end) = spec.split_once('-').unwrap();
                let start: usize = start.parse().unwrap();
                let end: usize = end.parse().unwrap();
                ResponseTemplate::new(206).set_body_bytes(body_for_mock[start..=end].to_vec())
            })
            .mount(&server)
            .await;

        let dest = tmp_dest("retry");
        let downloaded = download(&reqwest::Client::new(), &server.uri(), &dest, |_, _| {})
            .await
            .unwrap();

        assert_eq!(downloaded, total_size);
        assert_eq!(std::fs::read(&dest).unwrap(), full_body);
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
