use std::sync::Arc;

use weave_domain::persona::{builtin_personas, Persona};

use crate::{
    error::{AppError, AppResult},
    ports::persona_repository::PersonaRepository,
};

pub struct PersonaUseCase {
    repo: Arc<dyn PersonaRepository>,
}

impl PersonaUseCase {
    pub fn new(repo: Arc<dyn PersonaRepository>) -> Self {
        Self { repo }
    }

    /// Vrátí všechny persony: vestavěné + uživatelské.
    pub async fn list_all(&self) -> AppResult<Vec<Persona>> {
        let mut all = builtin_personas();
        all.extend(self.repo.list_custom().await?);
        Ok(all)
    }

    /// Najde personu podle ID (vestavěnou i vlastní).
    pub async fn find(&self, id: &str) -> AppResult<Option<Persona>> {
        if Persona::is_builtin(id) {
            return Ok(builtin_personas().into_iter().find(|p| p.id == id));
        }
        self.repo.find_by_id(id).await
    }

    /// Vytvoří vlastní personu.
    pub async fn create(
        &self,
        name: String,
        icon: String,
        system_prompt: String,
    ) -> AppResult<Persona> {
        if name.trim().is_empty() {
            return Err(AppError::Domain(
                weave_domain::error::DomainError::InvalidArgument(
                    "Jméno persony je prázdné".into(),
                ),
            ));
        }
        if system_prompt.trim().is_empty() {
            return Err(AppError::Domain(
                weave_domain::error::DomainError::InvalidArgument(
                    "System prompt je prázdný".into(),
                ),
            ));
        }
        let icon = if icon.trim().is_empty() {
            "🎭".to_string()
        } else {
            icon
        };
        let persona = Persona::new_custom(name, icon, system_prompt);
        self.repo.save(&persona).await?;
        Ok(persona)
    }

    /// Smaže vlastní personu (vestavěné nelze).
    pub async fn delete(&self, id: &str) -> AppResult<()> {
        if Persona::is_builtin(id) {
            return Err(AppError::Domain(
                weave_domain::error::DomainError::Forbidden(
                    "Vestavěnou personu nelze smazat".into(),
                ),
            ));
        }
        self.repo.delete(id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::persona_repository::MockPersonaRepository;

    #[tokio::test]
    async fn list_all_includes_builtins_and_custom() {
        let mut repo = MockPersonaRepository::new();
        repo.expect_list_custom().returning(|| {
            Box::pin(async { Ok(vec![Persona::new_custom("Můj", "🧪", "prompt")]) })
        });

        let uc = PersonaUseCase::new(Arc::new(repo));
        let all = uc.list_all().await.unwrap();
        assert_eq!(all.len(), 5); // 4 vestavěné + 1 vlastní
    }

    #[tokio::test]
    async fn create_rejects_empty_name() {
        let uc = PersonaUseCase::new(Arc::new(MockPersonaRepository::new()));
        assert!(uc
            .create("".into(), "🎭".into(), "prompt".into())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn delete_builtin_is_forbidden() {
        let uc = PersonaUseCase::new(Arc::new(MockPersonaRepository::new()));
        assert!(uc.delete("builtin:assistant").await.is_err());
    }

    #[tokio::test]
    async fn find_builtin_returns_from_domain() {
        let uc = PersonaUseCase::new(Arc::new(MockPersonaRepository::new()));
        let p = uc.find("builtin:coder").await.unwrap();
        assert!(p.is_some());
        assert_eq!(p.unwrap().name, "Kodér");
    }
}
