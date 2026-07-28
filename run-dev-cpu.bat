@echo off
REM Spusti Weave ve vyvojovem rezimu s vestavenou inferenci bezici jen na CPU.
REM
REM Proti run-dev.bat (CUDA) a run-dev-vulkan.bat nepotrebuje zadne GPU SDK --
REM staci CMake a Visual Studio (MSVC) s C++ workloadem. Hodi se, kdyz
REM   - nemas nainstalovanou CUDU / Vulkan SDK,
REM   - GPU build z nejakeho duvodu selhava a chces overit zbytek aplikace,
REM   - pracujes na UI a na rychlosti generovani nezalezi.
REM
REM Bezi to vyrazne pomaleji nez na GPU. NPU (OpenVINO) backend timhle
REM ovlivneny neni -- ten bezi ve vlastnim procesu a funguje i tady.
REM
REM Model (.gguf) se nastavuje v aplikaci:
REM   Nastaveni -> AI model -> Vestavena GPU inference

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

REM --- sqlx pouziva commitnutou offline cache, DB neni pri buildu potreba ---
set "SQLX_OFFLINE=true"

echo.
echo === Weave dev (CPU build, bez GPU akcelerace) ===
echo.

where pnpm >nul 2>&1
if errorlevel 1 (
    echo CHYBA: 'pnpm' nebyl nalezen v PATH.
    echo Zavri toto okno, zavri VSECHNY terminaly/okna a otevri novy terminal
    echo ^(PATH se nacita jen pri otevreni okna^), pak to zkus znovu.
    echo Kdyby pnpm chybel uplne: npm install -g pnpm
    pause
    endlocal
    exit /b 1
)

call pnpm tauri dev --features llm-embedded
set "EXITCODE=%errorlevel%"
if not "%EXITCODE%"=="0" (
    echo.
    echo === Build/spusteni selhalo ^(kod %EXITCODE%^) - viz vypis vyse ===
)

echo.
pause
endlocal & exit /b %EXITCODE%
