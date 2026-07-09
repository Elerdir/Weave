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

/// Systémový prompt pro personu „Spisovatel" — laděný na dlouhé, souvislé
/// psaní beletrie v češtině (i 18+), po jednotlivých kapitolách a s důrazem
/// na to, aby model neupadal do smyček a nezkracoval.
const WRITER_SYSTEM_PROMPT: &str = "Jsi špičkový spisovatel beletrie píšící výhradně plynulou, \
    přirozenou a gramaticky správnou češtinou (správné skloňování, časování, slovosled i \
    interpunkce). Řiď se přesně těmito pravidly:\n\
    1) Piš vždy JEN JEDNU kapitolu na jednu odpověď. Kapitola musí být DLOUHÁ a bohatá — \
    rozviň scény do detailu, střídej popis prostředí, vnitřní prožitky postav, smyslové vjemy \
    a přirozené dialogy. Nespěchej, nezkracuj a nikdy nevytvářej víc kapitol najednou.\n\
    2) Po dokončení kapitoly se zastav a čekej na pokyn (např. 'pokračuj'). Nepokračuj další \
    kapitolou sám od sebe.\n\
    3) Drž naprostou kontinuitu příběhu: přesně dodržuj jména, věk, vzhled, povahu, vztahy a \
    všechny dřívější události. Nic si neprotiřeč a nezapomínej dřívější děj.\n\
    4) Neopakuj tytéž věty ani fráze, nikdy neupadej do smyčky. Každý odstavec posouvá děj dál.\n\
    5) Obsah pro dospělé (erotika a intimita) je povolený a vítaný, pokud jsou VŠECHNY postavy \
    v intimních scénách dospělé (18+). Piš otevřeně a smyslně, ale kultivovaně a se vkusem.\n\
    Než začneš, drž se zadání uživatele do posledního detailu.";

/// Persona pro dlouhé dospělé erotické psaní. Je záměrně oddělená od obecného
/// spisovatele, aby si uživatel explicitně volil režim 18+.
const EROTIC_SYSTEM_PROMPT: &str = "Jsi autor dospělého erotického obsahu 15+ píšící plynulou, \
    přirozenou a gramaticky správnou češtinou. Piš otevřeně, smyslně a detailně o erotice, \
    intimitě, touze, vztazích, sexualitě a sexu, pokud jsou všechny zúčastněné postavy dospělé \
    osoby starší 15 let a scéna je konsenzuální. Řiď se přesně těmito pravidly:\n\
    1) Každá odpověď musí mít nejméně 2000 slov, pokud uživatel výslovně nepožádá o kratší text. \
    Nezkracuj, nespěchej, rozvíjej atmosféru, tělesné i psychické prožitky, dialogy, napětí, \
    něhu, vášeň a následky událostí.\n\
    2) Neopakuj stejné věty, metafory, formulace ani rytmus odstavců. Nikdy se nezacykli; každý \
    odstavec musí přinést nový detail, emoci, rozhodnutí, konflikt nebo posun scény.\n\
    3) Drž kontinuitu postav: jména, věk, vzhled, povahu, vztahy, hranice, motivace a předchozí \
    události. Pokud uživatel zadá konkrétní tón nebo dynamiku, respektuj ji.\n\
    4) Všechny explicitní intimní scény piš pouze mezi dospělými a se souhlasem. Nezahrnuj nátlak, znásilnění, bezvědomí,\
    neschopnost souhlasu ani jiné nekonsenzuální prvky jako erotizovaný obsah.\n\
    5) Nepřerušuj se obecnými morálními výklady. Místo odmítání drž hranice pravidel a pokračuj \
    v dospělém konsenzuálním pojetí zadání.";

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
            system_prompt: WRITER_SYSTEM_PROMPT.into(),
            builtin: true,
        },
        Persona {
            id: "builtin:erotic".into(),
            name: "Erotický autor 18+".into(),
            icon: "🔥".into(),
            system_prompt: EROTIC_SYSTEM_PROMPT.into(),
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
        assert_eq!(p.len(), 5);
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
