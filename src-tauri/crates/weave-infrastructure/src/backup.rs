//! Záloha a obnova uživatelských dat.
//!
//! Export: ZIP s konzistentním snapshotem databáze (`VACUUM INTO` — funguje
//! i za běhu aplikace) + složkou referenčních fotek (`reference-images`,
//! tj. i fotky postav). Galerie vygenerovaných obrázků se záměrně nebalí —
//! bývají to gigabajty a obrázky jdou uložit jednotlivě.
//!
//! Import: ZIP se rozbalí do `restore-pending/` a skutečná výměna souborů
//! proběhne až při dalším startu aplikace ([`apply_pending_restore`]) —
//! otevřenou SQLite databázi nelze bezpečně přepsat za běhu.

use std::io::Write;
use std::path::Path;

use weave_application::error::{AppError, AppResult};

const DB_FILE: &str = "weave.db";
const IMAGES_DIR: &str = "reference-images";
const STAGING_DIR: &str = "restore-pending";
const MANIFEST_FILE: &str = "weave-backup.json";

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::Repository(e.to_string())
}

/// Vytvoří zálohu do `dest_zip`. Vrací velikost výsledného souboru v bajtech.
pub async fn create_backup(
    pool: &sqlx::SqlitePool,
    data_dir: &Path,
    dest_zip: &Path,
) -> AppResult<u64> {
    // Konzistentní snapshot DB i za běhu. VACUUM INTO nepodporuje bind
    // parametry — cesta se vkládá jako SQL literál (apostrofy zdvojené).
    let snapshot = data_dir.join(format!("weave-backup-{}.db", uuid::Uuid::new_v4()));
    let snapshot_sql = snapshot.to_string_lossy().replace('\'', "''");
    sqlx::query(&format!("VACUUM INTO '{snapshot_sql}'"))
        .execute(pool)
        .await
        .map_err(err)?;

    let result = write_zip(data_dir, &snapshot, dest_zip);
    let _ = std::fs::remove_file(&snapshot);
    result
}

fn write_zip(data_dir: &Path, db_snapshot: &Path, dest_zip: &Path) -> AppResult<u64> {
    let file = std::fs::File::create(dest_zip).map_err(err)?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);

    // Manifest — hlavně pro budoucí kompatibilitu formátu.
    zip.start_file(MANIFEST_FILE, options).map_err(err)?;
    zip.write_all(
        serde_json::json!({
            "format": 1,
            "app": "weave",
            "created_at": chrono::Utc::now().to_rfc3339(),
        })
        .to_string()
        .as_bytes(),
    )
    .map_err(err)?;

    // Databáze (konverzace, postavy, nastavení, persony…)
    zip.start_file(DB_FILE, options).map_err(err)?;
    let mut db = std::fs::File::open(db_snapshot).map_err(err)?;
    std::io::copy(&mut db, &mut zip).map_err(err)?;

    // Referenční fotky (chat přílohy + fotky postav)
    let images_root = data_dir.join("weave").join(IMAGES_DIR);
    if images_root.is_dir() {
        for entry in std::fs::read_dir(&images_root).map_err(err)? {
            let entry = entry.map_err(err)?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            zip.start_file(format!("{IMAGES_DIR}/{name}"), options)
                .map_err(err)?;
            let mut f = std::fs::File::open(&path).map_err(err)?;
            std::io::copy(&mut f, &mut zip).map_err(err)?;
        }
    }

    zip.finish().map_err(err)?;
    Ok(std::fs::metadata(dest_zip).map_err(err)?.len())
}

/// Ověří ZIP a rozbalí ho do `restore-pending/` — výměnu provede až
/// [`apply_pending_restore`] při dalším startu aplikace.
pub fn stage_restore(data_dir: &Path, src_zip: &Path) -> AppResult<()> {
    let file = std::fs::File::open(src_zip).map_err(err)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| AppError::Repository(format!("Soubor není platný ZIP archiv: {e}")))?;

    if archive.by_name(DB_FILE).is_err() {
        return Err(AppError::Repository(
            "Archiv nevypadá jako záloha Weave (chybí weave.db).".into(),
        ));
    }

    let staging = data_dir.join(STAGING_DIR);
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(err)?;
    }
    std::fs::create_dir_all(&staging).map_err(err)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(err)?;
        // Ochrana proti zip-slip: enclosed_name odmítne cesty s `..`.
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let dest = staging.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).map_err(err)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        let mut out = std::fs::File::create(&dest).map_err(err)?;
        std::io::copy(&mut entry, &mut out).map_err(err)?;
    }
    Ok(())
}

/// Při startu aplikace (PŘED otevřením DB poolu) aplikuje připravenou obnovu:
/// nahradí `weave.db` a složku referenčních fotek obsahem `restore-pending/`.
/// Vrací `true`, když se obnova provedla.
pub fn apply_pending_restore(data_dir: &Path) -> AppResult<bool> {
    let staging = data_dir.join(STAGING_DIR);
    let staged_db = staging.join(DB_FILE);
    if !staged_db.is_file() {
        return Ok(false);
    }

    // DB: stará se odsune stranou (poslední záchrana), nová na její místo.
    let live_db = data_dir.join(DB_FILE);
    if live_db.exists() {
        let bak = data_dir.join("weave.db.pre-restore");
        let _ = std::fs::remove_file(&bak);
        std::fs::rename(&live_db, &bak).map_err(err)?;
        // WAL/SHM soubory staré DB už nesmí ovlivnit novou.
        let _ = std::fs::remove_file(data_dir.join("weave.db-wal"));
        let _ = std::fs::remove_file(data_dir.join("weave.db-shm"));
    }
    std::fs::rename(&staged_db, &live_db).map_err(err)?;

    // Referenční fotky: nahradit jen když je záloha obsahovala.
    let staged_images = staging.join(IMAGES_DIR);
    if staged_images.is_dir() {
        let live_images = data_dir.join("weave").join(IMAGES_DIR);
        if live_images.exists() {
            std::fs::remove_dir_all(&live_images).map_err(err)?;
        }
        if let Some(parent) = live_images.parent() {
            std::fs::create_dir_all(parent).map_err(err)?;
        }
        std::fs::rename(&staged_images, &live_images).map_err(err)?;
    }

    let _ = std::fs::remove_dir_all(&staging);
    tracing::info!("Záloha obnovena (weave.db + referenční fotky)");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("weave_backup_{name}_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn backup_and_restore_roundtrip() {
        // Zdrojová "instalace": skutečná DB (migrace) + jedna referenční fotka
        let src = tmp_dir("src");
        let url = format!("sqlite://{}", src.join(DB_FILE).to_string_lossy());
        let pool = crate::db::create_pool(&url).await.unwrap();
        let images = src.join("weave").join(IMAGES_DIR);
        std::fs::create_dir_all(&images).unwrap();
        std::fs::write(images.join("nikol.png"), b"fake-photo").unwrap();

        // Export
        let zip_path = src.join("zaloha.zip");
        let size = create_backup(&pool, &src, &zip_path).await.unwrap();
        assert!(size > 0);
        assert!(zip_path.exists());
        // Snapshot po sobě uklidil
        assert!(!std::fs::read_dir(&src)
            .unwrap()
            .flatten()
            .any(|e| { e.file_name().to_string_lossy().starts_with("weave-backup-") }));

        // Import do čerstvé "instalace"
        let dst = tmp_dir("dst");
        std::fs::write(dst.join(DB_FILE), b"stara databaze").unwrap();
        stage_restore(&dst, &zip_path).unwrap();
        assert!(dst.join(STAGING_DIR).join(DB_FILE).exists());

        let applied = apply_pending_restore(&dst).unwrap();
        assert!(applied);
        // Nová DB je platná SQLite (začíná magic headerem), stará odsunutá
        let header = std::fs::read(dst.join(DB_FILE)).unwrap();
        assert!(header.starts_with(b"SQLite format 3"));
        assert!(dst.join("weave.db.pre-restore").exists());
        // Fotky na svém místě, staging uklizený
        assert_eq!(
            std::fs::read(dst.join("weave").join(IMAGES_DIR).join("nikol.png")).unwrap(),
            b"fake-photo"
        );
        assert!(!dst.join(STAGING_DIR).exists());

        // Bez stagingu je apply no-op
        assert!(!apply_pending_restore(&dst).unwrap());

        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn stage_restore_rejects_non_backup_zip() {
        let dir = tmp_dir("reject");
        // ZIP bez weave.db
        let zip_path = dir.join("cizi.zip");
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> = Default::default();
            zip.start_file("readme.txt", options).unwrap();
            zip.write_all(b"hello").unwrap();
            zip.finish().unwrap();
        }
        let e = stage_restore(&dir, &zip_path).unwrap_err().to_string();
        assert!(e.contains("weave.db"), "{e}");

        // Ne-ZIP soubor
        let not_zip = dir.join("neni.zip");
        std::fs::write(&not_zip, b"tohle neni zip").unwrap();
        assert!(stage_restore(&dir, &not_zip).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
