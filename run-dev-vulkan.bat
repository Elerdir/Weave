@echo off
REM Spusti Weave ve vyvojovem rezimu s Vulkan GPU inferenci (llama.cpp).
REM Urceno pro Windows s AMD/Intel GPU (pro NVIDIA pouzij run-dev.bat s CUDA).
REM
REM Predpoklady:
REM   - Vulkan SDK (https://vulkan.lunarg.com/sdk/home) -- build potrebuje
REM     glslc a hlavicky; instalator nastavi promennou VULKAN_SDK
REM   - CMake, Visual Studio (MSVC) s C++ workloadem
REM
REM Model (.gguf) a pocet GPU vrstev se nastavuji v aplikaci:
REM   Nastaveni -> AI model -> Vestavena GPU inference

setlocal

if not defined VULKAN_SDK (
    echo CHYBA: promenna VULKAN_SDK neni nastavena -- nainstaluj Vulkan SDK
    echo z https://vulkan.lunarg.com/sdk/home a otevri novy terminal.
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
    pause
    endlocal
    exit /b 1
)

call pnpm tauri dev --features llm-vulkan
if errorlevel 1 (
    echo.
    echo === Build/spusteni selhalo ^(kod %errorlevel%^) — viz vypis vyse ===
)

echo.
pause
endlocal
