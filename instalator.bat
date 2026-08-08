@echo off
REM Sestavi plnohodnotny Windows MSI instalator Weave (release build).
REM
REM Stavi se S feature llm-vulkan, protoze vestavena inference je jediny
REM backend, ktery appka ma -- bez ni by nainstalovana aplikace neumela
REM chatovat vubec. Vulkan SDK potrebuje jen TENHLE stroj pri buildu;
REM uzivatel instalatoru ne, protoze runtime (vulkan-1.dll) je soucasti
REM ovladace grafiky.
REM
REM Updater artefakty (.sig) tenhle skript vypina primo pres --config override,
REM aniz by se sahalo na tauri.conf.json - "createUpdaterArtifacts": true tam
REM vyzaduje TAURI_SIGNING_PRIVATE_KEY (podpisovy klic updateru), ktery pro
REM lokalni build nepotrebujes.
REM
REM Vznikne jeden .msi pro kazdy jazyk nastaveny v tauri.conf.json
REM (bundle.windows.wix.language = cs-CZ a en-US), takze soubory jsou dva.
REM
REM Vysledny .msi najdes v: target\release\bundle\msi\

setlocal enabledelayedexpansion

REM --- Pracuj vzdy v adresari skriptu, at uz se spusti odkudkoli ---
cd /d "%~dp0"

if not exist "package.json" (
    echo CHYBA: v "%CD%" neni package.json.
    echo Skript musi zustat v korenovem adresari repozitare Weave.
    pause
    endlocal
    exit /b 1
)

if not defined VULKAN_SDK (
    echo CHYBA: promenna VULKAN_SDK neni nastavena.
    echo Nainstaluj Vulkan SDK z https://vulkan.lunarg.com/sdk/home
    echo a otevri novy terminal ^(promenne se nacitaji pri otevreni okna^).
    pause
    endlocal
    exit /b 1
)

REM --- Visual Studio: kompilator do PATH pro Ninju ---
set "VCVARS="
for %%V in (18 17 16) do (
    for %%E in (Community Professional Enterprise BuildTools) do (
        if exist "%ProgramFiles%\Microsoft Visual Studio\%%V\%%E\VC\Auxiliary\Build\vcvars64.bat" (
            if not defined VCVARS set "VCVARS=%ProgramFiles%\Microsoft Visual Studio\%%V\%%E\VC\Auxiliary\Build\vcvars64.bat"
        )
    )
)
if not defined VCVARS (
    echo CHYBA: Visual Studio s C++ workloadem nenalezeno.
    pause
    endlocal
    exit /b 1
)
REM vcvars64 hleda vswhere.exe v PATH; kdyz tam neni, vypise matouci hlasku.
REM Pozor: uvnitr zavorkovaneho bloku se musi psat !ProgramFiles(x86)!.
if exist "!ProgramFiles(x86)!\Microsoft Visual Studio\Installer\vswhere.exe" (
    set "PATH=!ProgramFiles(x86)!\Microsoft Visual Studio\Installer;!PATH!"
)
call "%VCVARS%" >nul

REM --- Ninja misto MSBuildu ---
REM MSBuild pada pri kompilaci Vulkan shaderu ("cannot find the batch label
REM VCEnd"): llama-cpp-sys-2 si pro Vulkan vypina TrackFileAccess a to rozbije
REM paralelni custom build kroky. Ninja je soucasti VS.
set "NINJA_DIR="
for %%V in (18 17 16) do (
    for %%E in (Community Professional Enterprise BuildTools) do (
        if exist "%ProgramFiles%\Microsoft Visual Studio\%%V\%%E\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja\ninja.exe" (
            if not defined NINJA_DIR set "NINJA_DIR=%ProgramFiles%\Microsoft Visual Studio\%%V\%%E\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja"
        )
    )
)
if defined NINJA_DIR (
    set "PATH=%NINJA_DIR%;%PATH%"
    set "CMAKE_GENERATOR=Ninja"
) else (
    echo VAROVANI: Ninja nenalezen - build Vulkan shaderu nejspis spadne na MSBuildu.
)

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
echo === Weave - build MSI instalatoru (release, Vulkan) ===
echo VULKAN_SDK=%VULKAN_SDK%
echo GENERATOR=%CMAKE_GENERATOR%
echo.
echo Release build llama.cpp trva dele nez dev - pocitej s desitkami minut.
echo.

set "OVERRIDE=%TEMP%\weave-installer-override.json"
> "%OVERRIDE%" echo {"bundle":{"createUpdaterArtifacts":false}}

call pnpm tauri build --bundles msi --features llm-vulkan --config "%OVERRIDE%"
set "EXITCODE=%errorlevel%"
if not "%EXITCODE%"=="0" (
    echo.
    echo === Build MSI selhal ^(kod %EXITCODE%^) - viz vypis vyse ===
    pause
    REM %EXITCODE% se musi expandovat driv, nez ho endlocal zahodi -- proto "&".
    endlocal & exit /b %EXITCODE%
)

echo.
echo === Hotovo. MSI instalator najdes zde: ===
for %%f in (target\release\bundle\msi\*.msi) do echo   %%f

echo.
pause
endlocal
