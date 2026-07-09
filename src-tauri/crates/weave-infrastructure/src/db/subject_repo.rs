use sqlx::{Row, SqlitePool};
use weave_application::error::{AppError, AppResult};
use weave_domain::subject::{Subject, SubjectImage};

/// Úložiště referenčních postav. Používá runtime `sqlx::query` (ne `query!`
/// makro), aby nebylo potřeba obnovovat offline `.sqlx` cache.
pub struct SqliteSubjectRepository {
    pool: SqlitePool,
}

#[async_trait::async_trait]
impl weave_application::ports::subject_repository::SubjectRepository for SqliteSubjectRepository {
    async fn list(&self) -> AppResult<Vec<Subject>> {
        SqliteSubjectRepository::list(self).await
    }
}

impl SqliteSubjectRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn map_err(e: sqlx::Error) -> AppError {
        AppError::Repository(e.to_string())
    }

    /// Všechny postavy včetně jejich fotek (nejnovější dřív).
    pub async fn list(&self) -> AppResult<Vec<Subject>> {
        let subj_rows =
            sqlx::query("SELECT id, name, notes FROM subjects ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await
                .map_err(Self::map_err)?;

        let mut subjects = Vec::with_capacity(subj_rows.len());
        for row in subj_rows {
            let id: String = row.try_get("id").map_err(Self::map_err)?;
            let images = self.images_for(&id).await?;
            subjects.push(Subject {
                id,
                name: row.try_get("name").map_err(Self::map_err)?,
                notes: row.try_get("notes").map_err(Self::map_err)?,
                images,
            });
        }
        Ok(subjects)
    }

    async fn images_for(&self, subject_id: &str) -> AppResult<Vec<SubjectImage>> {
        let rows = sqlx::query(
            "SELECT id, path, mime FROM subject_images WHERE subject_id = ? ORDER BY created_at ASC",
        )
        .bind(subject_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Self::map_err)?;

        rows.into_iter()
            .map(|r| {
                Ok(SubjectImage {
                    id: r.try_get("id").map_err(Self::map_err)?,
                    path: r.try_get("path").map_err(Self::map_err)?,
                    mime: r.try_get("mime").map_err(Self::map_err)?,
                })
            })
            .collect()
    }

    /// Vytvoří novou postavu a vrátí ji.
    pub async fn create(&self, name: &str) -> AppResult<Subject> {
        let subject = Subject::new(name);
        let created_at = chrono::Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO subjects (id, name, notes, created_at) VALUES (?, ?, ?, ?)")
            .bind(&subject.id)
            .bind(&subject.name)
            .bind(&subject.notes)
            .bind(&created_at)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        Ok(subject)
    }

    pub async fn rename(&self, id: &str, name: &str) -> AppResult<()> {
        sqlx::query("UPDATE subjects SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        Ok(())
    }

    pub async fn set_notes(&self, id: &str, notes: &str) -> AppResult<()> {
        sqlx::query("UPDATE subjects SET notes = ? WHERE id = ?")
            .bind(notes)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        Ok(())
    }

    /// Smaže postavu (fotky se smažou kaskádou v DB). Vrací cesty souborů,
    /// aby je volající mohl smazat i z disku.
    pub async fn delete(&self, id: &str) -> AppResult<Vec<String>> {
        let paths: Vec<String> = self
            .images_for(id)
            .await?
            .into_iter()
            .map(|i| i.path)
            .collect();
        sqlx::query("DELETE FROM subjects WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        Ok(paths)
    }

    /// Přidá k postavě fotku (soubor už musí být ve spravovaném úložišti).
    pub async fn add_image(
        &self,
        subject_id: &str,
        path: &str,
        mime: &str,
    ) -> AppResult<SubjectImage> {
        let image = SubjectImage {
            id: format!("subjimg:{}", uuid::Uuid::new_v4()),
            path: path.to_string(),
            mime: mime.to_string(),
        };
        let created_at = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO subject_images (id, subject_id, path, mime, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&image.id)
        .bind(subject_id)
        .bind(&image.path)
        .bind(&image.mime)
        .bind(&created_at)
        .execute(&self.pool)
        .await
        .map_err(Self::map_err)?;
        Ok(image)
    }

    /// Odebere fotku z postavy a vrátí její cestu (ke smazání z disku).
    pub async fn remove_image(&self, image_id: &str) -> AppResult<Option<String>> {
        let path: Option<String> = sqlx::query("SELECT path FROM subject_images WHERE id = ?")
            .bind(image_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Self::map_err)?
            .map(|r| r.try_get("path"))
            .transpose()
            .map_err(Self::map_err)?;

        sqlx::query("DELETE FROM subject_images WHERE id = ?")
            .bind(image_id)
            .execute(&self.pool)
            .await
            .map_err(Self::map_err)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_repo() -> SqliteSubjectRepository {
        let dir = std::env::temp_dir().join(format!("weave_subjects_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let url = format!("sqlite://{}", dir.join("test.db").to_string_lossy());
        SqliteSubjectRepository::new(crate::db::create_pool(&url).await.unwrap())
    }

    #[tokio::test]
    async fn crud_subject_with_images() {
        let repo = test_repo().await;

        let s = repo.create("Nikol").await.unwrap();
        repo.set_notes(&s.id, "19 let, blond").await.unwrap();
        let img1 = repo
            .add_image(&s.id, "/refs/a.png", "image/png")
            .await
            .unwrap();
        repo.add_image(&s.id, "/refs/b.png", "image/png")
            .await
            .unwrap();

        let subjects = repo.list().await.unwrap();
        assert_eq!(subjects.len(), 1);
        let loaded = &subjects[0];
        assert_eq!(loaded.name, "Nikol");
        assert_eq!(loaded.notes, "19 let, blond");
        assert_eq!(loaded.images.len(), 2);

        // Odebrání jedné fotky vrátí její cestu.
        let removed = repo.remove_image(&img1.id).await.unwrap();
        assert_eq!(removed.as_deref(), Some("/refs/a.png"));
        assert_eq!(repo.list().await.unwrap()[0].images.len(), 1);

        // Smazání postavy vrátí cesty zbývajících fotek (ke smazání z disku)
        // a odstraní ji.
        let paths = repo.delete(&s.id).await.unwrap();
        assert_eq!(paths, vec!["/refs/b.png".to_string()]);
        assert!(repo.list().await.unwrap().is_empty());
    }
}
