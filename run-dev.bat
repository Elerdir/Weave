@echo off
REM Spusti Weave ve vyvojovem rezimu s vestavenou CUDA GPU inferenci (llama.cpp).
REM
REM Predpoklady:
REM   - CUDA Toolkit (skript si sam najde nejnovejsi nainstalovanou verzi)
REM   - CMake, Visual Studio (MSVC) s C++ workloadem
REM   - NVIDIA GPU
REM
REM Nemas NVIDII nebo nechces stavet CUDA? Pouzij:
REM   run-dev-cpu.bat    (jen CPU, nepotrebuje CUDA ani Vulkan SDK)
REM   run-dev-vulkan.bat   (AMD / Intel GPU)
REM
REM Chces jinou CUDA verzi nez tu nejnovejsi? Nastav si pred spustenim
REM promennou CUDA_PATH a skript ji respektuje.
REM
REM Model (.gguf) a pocet GPU vrstev se nastavuji v aplikaci:
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

REM --- CUDA Toolkit: respektuj CUDA_PATH, jinak najdi nejnovejsi nainstalovanou ---
set "CUDA_ROOT=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA"
if not defined CUDA_PATH (
    for /f "delims=" %%d in ('dir /b /ad /on "%CUDA_ROOT%\v*" 2^>nul') do set "CUDA_PATH=%CUDA_ROOT%\%%d"
)

if not exist "%CUDA_PATH%\bin\nvcc.exe" (
    echo CHYBA: nenasel jsem CUDA Toolkit ^(nvcc.exe^).
    echo Hledal jsem v: %CUDA_ROOT%\v*
    echo.
    echo Bud nainstaluj CUDA Toolkit z
    echo   https://developer.nvidia.com/cuda-downloads
    echo nebo pouzij run-dev-cpu.bat ^(jen CPU, bez CUDA^).
    pause
    endlocal
    exit /b 1
)

set "CUDACXX=%CUDA_PATH%\bin\nvcc.exe"
set "PATH=%CUDA_PATH%\bin;%PATH%"

REM --- Cilova GPU architektura: zeptej se karty, jinak fallback na 86 (RTX 30xx) ---
if not defined CMAKE_CUDA_ARCHITECTURES (
    for /f "usebackq tokens=1 delims=, " %%c in (`nvidia-smi --query-gpu^=compute_cap --format^=csv^,noheader 2^>nul`) do (
        if not defined CMAKE_CUDA_ARCHITECTURES set "CMAKE_CUDA_ARCHITECTURES=%%c"
    )
    set "CMAKE_CUDA_ARCHITECTURES=!CMAKE_CUDA_ARCHITECTURES:.=!"
)
if not defined CMAKE_CUDA_ARCHITECTURES set "CMAKE_CUDA_ARCHITECTURES=86"

REM --- sqlx pouziva commitnutou offline cache, DB se neni potreba pripojovat pri buildu ---
set "SQLX_OFFLINE=true"

echo.
echo === Weave dev (CUDA build) ===
echo CUDA_PATH=%CUDA_PATH%
echo CMAKE_CUDA_ARCHITECTURES=%CMAKE_CUDA_ARCHITECTURES%
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

call pnpm tauri dev --features llm-cuda
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
