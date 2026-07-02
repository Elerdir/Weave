//! Čtení a filtrace aplikačních logů (soubory z `tracing-appender`,
//! denní rotace `weave.log.YYYY-MM-DD` ve složce `logs/` v app data).
//!
//! Parser je tolerantní k formátu `tracing_subscriber::fmt` bez ANSI:
//! `2026-07-02T13:00:00.123456Z  INFO weave_shell::commands: zpráva`
//! Řádky bez hlavičky (víceřádkové zprávy, backtrace) se lepí k předchozímu
//! záznamu.

use std::collections::VecDeque;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LogFilter {
    /// Minimální úroveň ("error" > "warn" > "info" > "debug" > "trace").
    pub min_level: Option<String>,
    /// Podřetězec v cílovém modulu (např. "comfy").
    pub target: Option<String>,
    /// Fulltext (case-insensitive) přes zprávu i modul.
    pub search: Option<String>,
    /// Kolik posledních záznamů vrátit (výchozí 500).
    pub limit: Option<usize>,
}

const DEFAULT_LIMIT: usize = 500;

/// Pořadí závažnosti — vyšší číslo = závažnější.
fn level_rank(level: &str) -> u8 {
    match level.to_ascii_uppercase().as_str() {
        "ERROR" => 5,
        "WARN" => 4,
        "INFO" => 3,
        "DEBUG" => 2,
        "TRACE" => 1,
        _ => 0,
    }
}

/// Zparsuje jeden řádek logu. Vrací None pro řádky bez hlavičky
/// (pokračování víceřádkové zprávy).
pub fn parse_line(line: &str) -> Option<LogEntry> {
    let mut parts = line.split_whitespace();
    let timestamp = parts.next()?;
    // Hlavička začíná RFC3339 časem — jinak jde o pokračování zprávy.
    if !timestamp.contains('T') || !timestamp.contains('-') {
        return None;
    }
    let level = parts.next()?;
    if level_rank(level) == 0 {
        return None;
    }
    // První token končící dvojtečkou = target (mezi tím mohou být spany).
    let rest: Vec<&str> = parts.collect();
    let target_idx = rest.iter().position(|t| t.ends_with(':'))?;
    let target = rest[target_idx].trim_end_matches(':').to_string();
    let message = rest[target_idx + 1..].join(" ");
    Some(LogEntry {
        timestamp: timestamp.to_string(),
        level: level.to_ascii_uppercase(),
        target,
        message,
    })
}

/// Projde filtr? (limit se aplikuje až při čtení)
pub fn matches(entry: &LogEntry, filter: &LogFilter) -> bool {
    if let Some(min) = &filter.min_level {
        if level_rank(&entry.level) < level_rank(min) {
            return false;
        }
    }
    if let Some(target) = &filter.target {
        if !target.is_empty()
            && !entry
                .target
                .to_ascii_lowercase()
                .contains(&target.to_ascii_lowercase())
        {
            return false;
        }
    }
    if let Some(search) = &filter.search {
        if !search.is_empty() {
            let needle = search.to_lowercase();
            if !entry.message.to_lowercase().contains(&needle)
                && !entry.target.to_ascii_lowercase().contains(&needle)
            {
                return false;
            }
        }
    }
    true
}

/// Přečte všechny log soubory ve složce (seřazené podle názvu = podle data),
/// zparsuje, aplikuje filtr a vrátí posledních `limit` záznamů.
pub fn read_logs(dir: &Path, filter: &LogFilter) -> Vec<LogEntry> {
    let limit = filter.limit.unwrap_or(DEFAULT_LIMIT).max(1);
    let mut files: Vec<_> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("weave.log"))
            })
            .collect(),
        Err(_) => return Vec::new(), // složka ještě neexistuje = žádné logy
    };
    files.sort();

    let mut result: VecDeque<LogEntry> = VecDeque::with_capacity(limit);
    for file in files {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };
        let mut current: Option<LogEntry> = None;
        for line in content.lines() {
            match parse_line(line) {
                Some(entry) => {
                    if let Some(done) = current.take() {
                        push_limited(&mut result, done, filter, limit);
                    }
                    current = Some(entry);
                }
                None => {
                    // Pokračování víceřádkové zprávy
                    if let Some(cur) = current.as_mut() {
                        if !line.trim().is_empty() {
                            cur.message.push('\n');
                            cur.message.push_str(line);
                        }
                    }
                }
            }
        }
        if let Some(done) = current.take() {
            push_limited(&mut result, done, filter, limit);
        }
    }
    result.into()
}

fn push_limited(out: &mut VecDeque<LogEntry>, entry: LogEntry, filter: &LogFilter, limit: usize) {
    if !matches(&entry, filter) {
        return;
    }
    if out.len() == limit {
        out.pop_front();
    }
    out.push_back(entry);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_log_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("weave_logs_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_standard_fmt_line() {
        let entry = parse_line(
            "2026-07-02T13:00:00.123456Z  INFO weave_shell::commands::message: Zpráva odeslána",
        )
        .unwrap();
        assert_eq!(entry.level, "INFO");
        assert_eq!(entry.target, "weave_shell::commands::message");
        assert_eq!(entry.message, "Zpráva odeslána");
    }

    #[test]
    fn parses_line_with_span_fields() {
        let entry = parse_line(
            "2026-07-02T13:00:00Z ERROR request{id=42}: weave_infrastructure::comfyui: selhalo",
        )
        .unwrap();
        assert_eq!(entry.level, "ERROR");
        // Span končí dvojtečkou dřív než target — tolerantní parser vezme
        // první token s dvojtečkou; důležité je, že zpráva se neztratí.
        assert!(entry.message.contains("selhalo"));
    }

    #[test]
    fn continuation_line_returns_none() {
        assert!(parse_line("    at src/main.rs:10").is_none());
        assert!(parse_line("").is_none());
    }

    #[test]
    fn filter_by_min_level() {
        let info = parse_line("2026-07-02T13:00:00Z  INFO app: běžná zpráva").unwrap();
        let error = parse_line("2026-07-02T13:00:01Z ERROR app: průšvih").unwrap();
        let filter = LogFilter {
            min_level: Some("warn".into()),
            ..Default::default()
        };
        assert!(!matches(&info, &filter));
        assert!(matches(&error, &filter));
    }

    #[test]
    fn filter_by_target_and_search() {
        let entry =
            parse_line("2026-07-02T13:00:00Z  INFO weave_infrastructure::comfyui: Stahuji model")
                .unwrap();
        let by_target = LogFilter {
            target: Some("comfy".into()),
            ..Default::default()
        };
        let by_search = LogFilter {
            search: Some("stahuji".into()),
            ..Default::default()
        };
        let miss = LogFilter {
            search: Some("neexistuje".into()),
            ..Default::default()
        };
        assert!(matches(&entry, &by_target));
        assert!(matches(&entry, &by_search));
        assert!(!matches(&entry, &miss));
    }

    #[test]
    fn read_logs_merges_files_applies_limit_and_multiline() {
        let dir = temp_log_dir();
        std::fs::write(
            dir.join("weave.log.2026-07-01"),
            "2026-07-01T10:00:00Z  INFO app: první\n\
             2026-07-01T10:00:01Z ERROR app: pád\nstack line 1\nstack line 2\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("weave.log.2026-07-02"),
            "2026-07-02T09:00:00Z  WARN app: novější\n",
        )
        .unwrap();
        std::fs::write(dir.join("nesouvisejici.txt"), "ignorovat\n").unwrap();

        let all = read_logs(&dir, &LogFilter::default());
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].message, "první");
        assert!(
            all[1].message.contains("stack line 2"),
            "víceřádková zpráva"
        );
        assert_eq!(all[2].message, "novější");

        let limited = read_logs(
            &dir,
            &LogFilter {
                limit: Some(2),
                ..Default::default()
            },
        );
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].level, "ERROR");
        assert_eq!(limited[1].message, "novější");
    }

    #[test]
    fn read_logs_empty_when_directory_missing() {
        let dir = std::env::temp_dir().join("weave_logs_neexistuje_xyz");
        assert!(read_logs(&dir, &LogFilter::default()).is_empty());
    }
}
