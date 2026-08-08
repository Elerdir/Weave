@echo off
REM Spusti Weave ve vyvojovem rezimu s vestavenou GPU inferenci pres Vulkan.
REM
REM Vulkan je hlavni cesta pro VSECHNY karty -- NVIDIA, AMD i Intel. CUDA se
REM pro text uz nestavi: u modelu vetsich nez VRAM nerozhoduje backend, ale
REM rozlozeni modelu (experti v RAM, attention na GPU -- viz llm::offload_plan),
REM a Vulkan SDK je proti CUDA Toolkitu o rad mensi. CUDA v projektu zustava
REM jen pro ComfyUI, ktere si ji instaluje samo do vlastniho Python prostredi.
REM
REM Predpoklady:
REM   - Vulkan SDK (https://vulkan.lunarg.com/sdk/home) -- instalator nastavi
REM     promennou VULKAN_SDK; build z nej potrebuje glslc a hlavicky
REM   - CMake, Visual Studio (MSVC) s C++ workloadem
REM
REM Nechces stavet GPU backend? Pouzij run-dev-cpu.bat (jen CPU).
REM
REM Model (.gguf) se nastavuje v aplikaci:
REM   Nastaveni -> AI model -> Vestavena GPU inference
REM Pocet vrstev na GPU uz nastavovat nemusis -- appka si rozlozeni spocita
REM sama podle velikosti modelu, GGUF hlavicky a volne VRAM.

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
    echo.
    echo Nez SDK nainstalujes, muzes zatim pouzit run-dev-cpu.bat.
    pause
    endlocal
    exit /b 1
)

REM --- sqlx pouziva commitnutou offline cache, DB neni pri buildu potreba ---
set "SQLX_OFFLINE=true"

echo.
echo === Weave dev (Vulkan build) ===
echo VULKAN_SDK=%VULKAN_SDK%
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

call pnpm tauri dev --features llm-vulkan
set "EXITCODE=%errorlevel%"
if not "%EXITCODE%"=="0" (
    echo.
    echo === Build/spusteni selhalo ^(kod %EXITCODE%^) - viz vypis vyse ===
)

echo.
pause
REM Pozor: %EXITCODE% se musi expandovat driv, nez endlocal promennou zahodi,
REM proto jsou oba prikazy na jednom radku spojene pres "&".
endlocal & exit /b %EXITCODE%
