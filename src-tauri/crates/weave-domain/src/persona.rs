use serde::{Deserialize, Serialize};

/// Persona = pojmenovaný system prompt (asistent s charakterem).
/// Vestavěné persony mají `builtin = true` a stabilní ID `builtin:*`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Persona {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub system_prompt: String,
    pub builtin: bool,
}

impl Persona {
    pub fn new_custom(
        name: impl Into<String>,
        icon: impl Into<String>,
        system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            id: format!("custom:{}", uuid::Uuid::new_v4()),
            name: name.into(),
            icon: icon.into(),
            system_prompt: system_prompt.into(),
            builtin: false,
        }
    }

    pub fn is_builtin(id: &str) -> bool {
        id.starts_with("builtin:")
    }
}

/// Vestavěné persony — doménová znalost, dostupné vždy.
pub fn builtin_personas() -> Vec<Persona> {
    vec![
        Persona {
            id: "builtin:assistant".into(),
            name: "Asistent".into(),
            icon: "🤖".into(),
            system_prompt: "Jsi nápomocný, věcný a přátelský asistent. Odpovídej jasně a stručně."
                .into(),
            builtin: true,
        },
        Persona {
            id: "builtin:writer".into(),
            name: "Spisovatel".into(),
            icon: "✍️".into(),
            system_prompt: "Jsi zkušený spisovatel beletrie. Piš poutavě, s citem pro atmosféru, \
                 dialogy a tempo vyprávění. Dbej na stylistickou čistotu."
                .into(),
            builtin: true,
        },
        Persona {
            id: "builtin:coder".into(),
            name: "Kodér".into(),
            icon: "💻".into(),
            system_prompt:
                "Jsi senior programátor. Piš čistý, idiomatický a dobře otestovaný kód. \
                 Vysvětluj rozhodnutí stručně a navrhuj nejlepší praktiky."
                    .into(),
            builtin: true,
        },
        Persona {
            id: "builtin:analyst".into(),
            name: "Analytik".into(),
            icon: "📊".into(),
            system_prompt: "Jsi pečlivý analytik. Rozkládej problémy na části, opírej se o fakta, \
                 uváděj předpoklady a zvažuj alternativy. Buď strukturovaný."
                .into(),
            builtin: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_have_stable_ids_and_prompts() {
        let p = builtin_personas();
        assert_eq!(p.len(), 4);
        assert!(p.iter().all(|x| x.builtin));
        assert!(p.iter().all(|x| x.id.starts_with("builtin:")));
        assert!(p.iter().all(|x| !x.system_prompt.is_empty()));
    }

    #[test]
    fn custom_persona_has_custom_id() {
        let c = Persona::new_custom("Test", "🧪", "prompt");
        assert!(c.id.starts_with("custom:"));
        assert!(!c.builtin);
        assert!(!Persona::is_builtin(&c.id));
    }
}
