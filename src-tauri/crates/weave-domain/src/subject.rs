use serde::{Deserialize, Serialize};

/// Referenční postava/subjekt — pojmenovaná osoba, ke které si uživatel uloží
/// několik fotek. Ty pak jedním klikem přiloží jako reference (PuLID) při
/// generování obrázků. `images` se plní při čtení z repozitáře.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Subject {
    pub id: String,
    pub name: String,
    pub notes: String,
    pub images: Vec<SubjectImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubjectImage {
    pub id: String,
    pub path: String,
    pub mime: String,
}

impl Subject {
    /// Nová prázdná postava se stabilním ID.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: format!("subject:{}", uuid::Uuid::new_v4()),
            name: name.into(),
            notes: String::new(),
            images: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_subject_has_stable_prefix_and_no_images() {
        let s = Subject::new("Nikol");
        assert!(s.id.starts_with("subject:"));
        assert_eq!(s.name, "Nikol");
        assert!(s.images.is_empty());
    }
}
