# Changelog

Významné změny Weave. Text sekcí jde jako „Co je nového" do UpdateHubu, k aktualizaci
v aplikaci a na www.niderle.cz — piš ho pro uživatele, ne pro vývojáře.

Formát vychází z [Keep a Changelog](https://keepachangelog.com/cs/1.1.0/), verzování drží
[SemVer](https://semver.org/lang/cs/). Verzi určuje tag vydání z Dispatche; před vydáním zapiš
změny pod `[Unreleased]`.

---

## [Unreleased]

### Opraveno
- Instalátor pro Windows teď na kartách NVIDIA RTX s velkou pamětí opravdu nainstaluje
  rychlejší akceleraci CUDA (dřív vždy zvolil Vulkan). U 12–19 GB paměti se zeptá, jinde
  zůstává Vulkan.
- Verze s CUDA se spouští se správným rozhraním aplikace.
