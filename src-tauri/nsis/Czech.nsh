; Český překlad vlastních textů Tauri instalátoru.
;
; POZOR: tenhle soubor NESMÍ mít BOM. Tauri si ho při kopírování do build
; adresáře přidá samo a dvojitý BOM makensis odmítne ("Invalid command").
; U nsis/gpu-backend.nsh je to naopak — ten se vkládá přímo ze zdrojů, tam
; BOM potřeba je, aby se česká diakritika nerozsypala.
;
; Tauri dodává tyhle řetězce jen pro část jazyků a čeština mezi nimi NENÍ —
; bez tohohle souboru zůstanou popisky prázdné a uživatel vidí stránku se
; dvěma nepopsanými přepínači (NSIS přeloží jen svá vlastní tlačítka
; Zpět/Další/Storno, ne texty aplikace).
;
; Zapojeno přes bundle.windows.nsis.customLanguageFiles v tauri.conf.json.
; Seznam řetězců odpovídá English.nsh z tauri-bundleru — když Tauri přidá
; další, projeví se zase jako prázdné místo.
;
; Zástupné znaky se MUSÍ zachovat: ${PRODUCTNAME} a ${VERSION} doplňuje NSIS,
; {{product_name}} dosadí Tauri při generování skriptu, $R4/$0/$1 jsou
; proměnné instalátoru a $\n je konec řádku.

LangString addOrReinstall ${LANG_CZECH} "Přidat nebo přeinstalovat součásti"
LangString alreadyInstalled ${LANG_CZECH} "Aplikace je už nainstalovaná"
LangString alreadyInstalledLong ${LANG_CZECH} "${PRODUCTNAME} ${VERSION} je už nainstalovaný. Vyber, co se má provést, a pokračuj tlačítkem Další."
LangString appRunning ${LANG_CZECH} "{{product_name}} právě běží. Zavři ho a zkus to znovu."
LangString appRunningOkKill ${LANG_CZECH} "{{product_name}} právě běží.$\nTlačítkem OK ho ukončíš."
LangString chooseMaintenanceOption ${LANG_CZECH} "Vyber, co se má provést."
LangString choowHowToInstall ${LANG_CZECH} "Vyber, jak se má ${PRODUCTNAME} nainstalovat."
LangString createDesktop ${LANG_CZECH} "Vytvořit zástupce na ploše"
LangString deleteAppData ${LANG_CZECH} "Smazat i data aplikace (konverzace, nastavení, obrázky)"
LangString dontUninstall ${LANG_CZECH} "Neodinstalovávat"
LangString dontUninstallDowngrade ${LANG_CZECH} "Neodinstalovávat (návrat na starší verzi bez odinstalace je v tomto instalátoru vypnutý)"
LangString failedToKillApp ${LANG_CZECH} "{{product_name}} se nepodařilo ukončit. Zavři ho ručně a zkus to znovu."
LangString installingWebview2 ${LANG_CZECH} "Instaluji WebView2…"
LangString newerVersionInstalled ${LANG_CZECH} "Je nainstalovaná novější verze aplikace ${PRODUCTNAME}. Instalovat starší se nedoporučuje; pokud to opravdu chceš, odinstaluj nejdřív současnou verzi. Vyber, co se má provést, a pokračuj tlačítkem Další."
LangString older ${LANG_CZECH} "starší"
LangString olderOrUnknownVersionInstalled ${LANG_CZECH} "V počítači je nainstalovaná $R4 verze aplikace ${PRODUCTNAME}. Doporučujeme ji před instalací odinstalovat. Vyber, co se má provést, a pokračuj tlačítkem Další."
LangString silentDowngrades ${LANG_CZECH} "Návrat na starší verzi je v tomto instalátoru vypnutý, takže tichá instalace nemůže pokračovat. Použij instalátor s grafickým rozhraním.$\n"
LangString unableToUninstall ${LANG_CZECH} "Odinstalace se nezdařila."
LangString uninstallApp ${LANG_CZECH} "Odinstalovat ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_CZECH} "Před instalací odinstalovat"
LangString unknown ${LANG_CZECH} "neznámá"
LangString webview2AbortError ${LANG_CZECH} "Instalace WebView2 selhala. Bez něj aplikace nepoběží — zkus instalátor spustit znovu."
LangString webview2DownloadError ${LANG_CZECH} "Chyba: stahování WebView2 selhalo – $0"
LangString webview2DownloadSuccess ${LANG_CZECH} "WebView2 stažen"
LangString webview2Downloading ${LANG_CZECH} "Stahuji WebView2…"
LangString webview2InstallError ${LANG_CZECH} "Chyba: instalace WebView2 selhala s kódem $1"
LangString webview2InstallSuccess ${LANG_CZECH} "WebView2 nainstalován"
