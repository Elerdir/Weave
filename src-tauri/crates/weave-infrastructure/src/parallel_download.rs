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
        download_sequential(http, url, dest, total, supports_range, on_progress).await
    } else {
        download_segmented(http, url, dest, total, on_progress).await
    };

    // Po selhání: když server umí Range, rozpracovaný soubor NECHÁVÁME —
    // příští pokus na něj naváže (sekvenčně od velikosti souboru, segmentovaně
    // podle sidecar metadat). Bez podpory Range je částečný soubor k ničemu
    // a soubor s dírami by se tvářil jako kompletní → smazat i s metadaty.
    if result.is_err() && !supports_range {
        let _ = std::fs::remove_file(dest);
        let _ = std::fs::remove_file(segments_meta_path(dest));
    }
    result
}

/// Sidecar soubor segmentovaného stahování — pamatuje si, které segmenty už
/// jsou komplet, aby šlo po přerušení navázat místo stahování od nuly.
fn segments_meta_path(dest: &Path) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{}.segments.json", dest.to_string_lossy()))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SegmentsMeta {
    total: u64,
    segment_size: u64,
    completed: Vec<bool>,
}

impl SegmentsMeta {
    fn load_if_resumable(dest: &Path, total: u64, segment_size: u64, count: usize) -> Option<Self> {
        let meta: SegmentsMeta =
            serde_json::from_str(&std::fs::read_to_string(segments_meta_path(dest)).ok()?).ok()?;
        // Layout musí sedět (jiná velikost/segmentace = jiný soubor či konfigurace)
        // a soubor musí existovat v plné předalokované velikosti.
        let file_len = dest.metadata().ok()?.len();
        (meta.total == total
            && meta.segment_size == segment_size
            && meta.completed.len() == count
            && file_len == total)
            .then_some(meta)
    }

    fn store(&self, dest: &Path) {
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(segments_meta_path(dest), json);
        }
    }
}

async fn download_sequential(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    total_hint: u64,
    supports_range: bool,
    on_progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<u64, String> {
    use std::io::{BufWriter, Write};

    // Resume: existující částečný soubor + Range podpora + známá cílová
    // velikost → pokračujeme od konce souboru místo stahování od nuly.
    // (Bez validace verze souboru — stejné zjednodušení jako běžné downloadery;
    // checksum po stažení případný nesoulad odhalí.)
    let existing = dest.metadata().map(|m| m.len()).unwrap_or(0);
    let resume_from = if supports_range && total_hint > 0 && existing > 0 && existing < total_hint {
        existing
    } else {
        0
    };

    let mut request = http.get(url);
    if resume_from > 0 {
        request = request.header(RANGE, format!("bytes={resume_from}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Stažení selhalo: {e}"))?;
    if response.error_for_status_ref().is_err() {
        return Err(format!("Server vrátil {}", response.status()));
    }

    // Server může resume ignorovat a poslat celé tělo (200) → začínáme od nuly.
    let resuming = resume_from > 0 && response.status() == StatusCode::PARTIAL_CONTENT;
    let (total, file, mut downloaded) = if resuming {
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(dest)
            .map_err(|e| e.to_string())?;
        (total_hint, file, resume_from)
    } else {
        let total = response.content_length().unwrap_or(total_hint);
        let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
        (total, file, 0u64)
    };
    let mut file = BufWriter::with_capacity(256 * 1024, file);

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
    let num_segments = (total / MIN_SEGMENT_SIZE).clamp(1, max_segments());
    let segment_size = total.div_ceil(num_segments);
    let segment_count = num_segments.min(total.div_ceil(segment_size)) as usize;

    // Resume: sidecar metadata z přerušeného stahování říkají, které segmenty
    // už jsou komplet — ty se přeskočí. Když metadata nesedí (jiná velikost,
    // jiný layout) nebo nejsou, soubor se předalokuje znovu a jede se od nuly.
    let meta = match SegmentsMeta::load_if_resumable(dest, total, segment_size, segment_count) {
        Some(meta) => meta,
        None => {
            // Předalokace na plnou velikost — segmenty pak píšou na svůj
            // offset nezávisle na sobě (žádný append).
            let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
            file.set_len(total).map_err(|e| e.to_string())?;
            SegmentsMeta {
                total,
                segment_size,
                completed: vec![false; segment_count],
            }
        }
    };

    // Metadata zapsat hned — kdyby appka spadla uprostřed (nebo selhaly
    // všechny segmenty), příští běh pozná rozpracované stahování a naváže.
    meta.store(dest);

    // Hotové segmenty se rovnou započítají do progresu.
    let initial: u64 = (0..segment_count)
        .filter(|&i| meta.completed[i])
        .map(|i| segment_len(i as u64, segment_size, total))
        .sum();
    let downloaded = Arc::new(AtomicU64::new(initial));
    let on_progress = Arc::new(on_progress);
    let meta = Arc::new(std::sync::Mutex::new(meta));

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
    for i in 0..segment_count as u64 {
        if meta.lock().expect("segments meta poisoned").completed[i as usize] {
            continue;
        }
        let start = i * segment_size;
        let end = (start + segment_size).min(total) - 1;
        let http = http.clone();
        let url = url.to_string();
        let dest = dest.to_path_buf();
        let downloaded = downloaded.clone();
        let meta = meta.clone();
        tasks.push(tokio::spawn(async move {
            download_range(&http, &url, &dest, start, end, &downloaded).await?;
            // Hotový segment hned zapsat do sidecar metadat — kdyby zbytek
            // selhal (nebo appka spadla), příští pokus tenhle segment přeskočí.
            let mut guard = meta.lock().expect("segments meta poisoned");
            guard.completed[i as usize] = true;
            guard.store(&dest);
            Ok::<(), String>(())
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
    // Kompletní soubor → sidecar metadata už nejsou potřeba.
    let _ = std::fs::remove_file(segments_meta_path(dest));
    on_progress(total, total);
    Ok(total)
}

/// Skutečná délka i-tého segmentu (poslední bývá kratší).
fn segment_len(i: u64, segment_size: u64, total: u64) -> u64 {
    let start = i * segment_size;
    (start + segment_size).min(total).saturating_sub(start)
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
    async fn sequential_resume_continues_from_partial_file() {
        let server = MockServer::start().await;
        let full_body: Vec<u8> = (0..1000u64).map(|i| (i % 256) as u8).collect();
        let cut = 400usize;

        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", full_body.len().to_string())
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;
        // Malý soubor (< MIN_PARALLEL_SIZE) → sekvenční cesta; server na
        // Range odpovídá 206 se zbytkem těla.
        let body_for_mock = full_body.clone();
        Mock::given(method("GET"))
            .respond_with(move |req: &wiremock::Request| {
                match req.headers.get("range").and_then(|v| v.to_str().ok()) {
                    Some(range) => {
                        let start: usize = range
                            .strip_prefix("bytes=")
                            .and_then(|s| s.strip_suffix('-'))
                            .unwrap()
                            .parse()
                            .unwrap();
                        ResponseTemplate::new(206).set_body_bytes(body_for_mock[start..].to_vec())
                    }
                    None => ResponseTemplate::new(200).set_body_bytes(body_for_mock.clone()),
                }
            })
            .mount(&server)
            .await;

        // Rozpracovaný soubor z „přerušeného" stahování
        let dest = tmp_dest("seq_resume");
        std::fs::write(&dest, &full_body[..cut]).unwrap();

        let downloaded = download(&reqwest::Client::new(), &server.uri(), &dest, |_, _| {})
            .await
            .unwrap();

        // Vrácený počet = celkem (včetně už existující části), obsah kompletní
        assert_eq!(downloaded, full_body.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), full_body);
        // A opravdu se navazovalo: přišel právě jeden GET s Range od místa řezu
        let range_gets = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| {
                r.method.as_str() == "GET"
                    && r.headers
                        .get("range")
                        .is_some_and(|v| v.to_str().unwrap() == format!("bytes={cut}-"))
            })
            .count();
        assert_eq!(range_gets, 1, "stahování mělo navázat od bajtu {cut}");
        let _ = std::fs::remove_file(&dest);
    }

    #[tokio::test]
    async fn segmented_resume_skips_completed_segments() {
        let server = MockServer::start().await;
        let total_size = 40 * 1024 * 1024u64; // 2 segmenty po 20 MB
        let full_body: Vec<u8> = (0..total_size).map(|i| (i % 256) as u8).collect();

        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", total_size.to_string())
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;

        // Druhý segment (od 20 MB) při prvním běhu trvale selhává — přesně
        // SEGMENT_ATTEMPTS pokusů, pak je mock vyčerpaný a další běh projde.
        let segment2_range = format!("bytes={}-{}", total_size / 2, total_size - 1);
        Mock::given(method("GET"))
            .and(wiremock::matchers::header("range", segment2_range.as_str()))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(u64::from(super::SEGMENT_ATTEMPTS))
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

        let dest = tmp_dest("seg_resume");

        // 1. běh: druhý segment selže → chyba, ale soubor + metadata zůstávají
        let first = download(&reqwest::Client::new(), &server.uri(), &dest, |_, _| {}).await;
        assert!(first.is_err());
        assert!(dest.exists(), "částečný soubor musí zůstat pro resume");
        assert!(
            segments_meta_path(&dest).exists(),
            "sidecar metadata musí zůstat pro resume"
        );

        // 2. běh: naváže — první (hotový) segment se už znovu nestahuje
        let downloaded = download(&reqwest::Client::new(), &server.uri(), &dest, |_, _| {})
            .await
            .unwrap();
        assert_eq!(downloaded, total_size);
        assert_eq!(std::fs::read(&dest).unwrap(), full_body);
        assert!(
            !segments_meta_path(&dest).exists(),
            "po úspěchu se metadata mažou"
        );

        // První segment stažen jen jednou (v 1. běhu), 2. běh ho přeskočil
        let segment1_range = format!("bytes=0-{}", total_size / 2 - 1);
        let segment1_gets = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| {
                r.headers
                    .get("range")
                    .is_some_and(|v| v.to_str().unwrap() == segment1_range)
            })
            .count();
        assert_eq!(segment1_gets, 1, "hotový segment se nesmí stahovat znovu");
        let _ = std::fs::remove_file(&dest);
    }

    #[tokio::test]
    async fn failed_segmented_download_keeps_file_and_meta_for_resume() {
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

        let dest = tmp_dest("failed_keep");
        let result = download(&reqwest::Client::new(), &server.uri(), &dest, |_, _| {}).await;

        assert!(result.is_err());
        // Server umí Range → rozpracovaný soubor + sidecar metadata zůstávají
        // pro resume. (Že se nedokončený soubor netváří jako hotový, řeší
        // volající vzorem `.part` + rename.)
        assert!(dest.exists(), "soubor má zůstat pro resume");
        assert!(
            segments_meta_path(&dest).exists(),
            "metadata mají zůstat pro resume"
        );
        let _ = std::fs::remove_file(&dest);
        let _ = std::fs::remove_file(segments_meta_path(&dest));
    }

    #[tokio::test]
    async fn failed_download_without_range_support_removes_partial_file() {
        // Bez podpory Range je částečný soubor k ničemu → po chybě se maže.
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(200).insert_header("content-length", "500"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let dest = tmp_dest("failed_noresume");
        let result = download(&reqwest::Client::new(), &server.uri(), &dest, |_, _| {}).await;

        assert!(result.is_err());
        assert!(!dest.exists(), "bez Range podpory se částečný soubor maže");
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
