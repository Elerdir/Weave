@echo off
REM Sestavi Windows MSI instalator Weave (release build, bez CUDA/embedded LLM
REM feature - stejne jako oficialni release, viz .github\workflows\release.yml).
REM
REM Updater artefakty (.sig) tenhle skript vypina primo pres --config override,
REM aniz by se sahalo na tauri.conf.json - "createUpdaterArtifacts": true tam
REM vyzaduje TAURI_SIGNING_PRIVATE_KEY (podpisovy klic updateru), ktery pro
REM lokalni testovaci build nepotrebujes.
REM
REM Vznikne jeden .msi pro kazdy jazyk nastaveny v tauri.conf.json
REM (bundle.windows.wix.language = cs-CZ a en-US), takze soubory jsou dva.
REM
REM Vysledny .msi najdes v: target\release\bundle\msi\

setlocal
set "SQLX_OFFLINE=true"

where pnpm >nul 2>&1
if errorlevel 1 (
    echo CHYBA: 'pnpm' nebyl nalezen v PATH.
    echo Zavri vsechny terminaly/okna a otevri novy ^(PATH se nacita jen pri
    echo otevreni okna^), pak to zkus znovu.
    pause
    endlocal
    exit /b 1
)

echo.
echo === Weave - build MSI instalatoru (release) ===
echo.

set "OVERRIDE=%TEMP%\weave-installer-override.json"
> "%OVERRIDE%" echo {"bundle":{"createUpdaterArtifacts":false}}

call pnpm tauri build --bundles msi --config "%OVERRIDE%"
if errorlevel 1 (
    echo.
    echo === Build MSI selhal ^(kod %errorlevel%^) - viz vypis vyse ===
    pause
    endlocal
    exit /b 1
)

echo.
echo === Hotovo. MSI instalator najdes zde: ===
for %%f in (target\release\bundle\msi\*.msi) do echo   %%f

echo.
pause
endlocal
