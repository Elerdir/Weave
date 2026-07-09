use async_trait::async_trait;
use weave_domain::subject::Subject;

use crate::error::AppResult;

/// Čtení referenčních postav pro orchestraci zpráv — když uživatel postavu
/// zmíní jménem, její poznámky jdou do kontextu psaní a fotky do generování.
/// (Plná CRUD správa postav žije v shell commands, use case potřebuje jen výpis.)
#[async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait SubjectRepository: Send + Sync {
    async fn list(&self) -> AppResult<Vec<Subject>>;
}
