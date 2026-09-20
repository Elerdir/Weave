@echo off
REM Sestavi Windows instalator Weave, ktery si pri instalaci sam vybere
REM GPU backend (viz src-tauri\nsis\gpu-backend.nsh):
REM
REM   NVIDIA >= 20 GB VRAM  -> CUDA
REM   NVIDIA 12-19 GB VRAM  -> zepta se uzivatele
REM   ostatni               -> Vulkan
REM
REM Proto se stavi DVE binarky (kazda s jinym backendem zakompilovanym
REM napevno) a obe se vlozi do jednoho instalatoru; ta nepotrebna se pri
REM instalaci smaze. Pocitej s ~1 GB instalatorem a hodinou buildu.
REM
REM Predpoklady: Vulkan SDK, CUDA Toolkit, Visual Studio s C++ workloadem,
REM CMake, pnpm. Uzivatel instalatoru nepotrebuje nic - Vulkan runtime je
REM v ovladaci grafiky a CUDA runtime (cuBLAS) je primo v balicku.
REM
REM Prepinac /nopause preskoci cekani na klavesu na konci. Bez nej nejde
REM skript spustit neinteraktivne: `pause` cte stdin, a kdyz se presmeruje
REM (treba z NUL nebo z CI), spadne na tom Ninja pri kompilaci llama.cpp
REM -- "ninja: fatal: ReadFile: The handle is invalid".
REM
REM Vysledek: target\release\bundle\nsis\*-setup.exe
REM (MSI tuhle detekci neumi - staveji se pres:
REM   pnpm tauri build --bundles msi --features llm-vulkan)

setlocal enabledelayedexpansion
cd /d "%~dp0"

REM /nopause = neceka na klavesu (pro CI a automatizaci).
REM Prochazi se pres shift, ne pres `for %%A in (%*)` -- ten tokeny
REM zacinajici lomitkem bere jako prepinace sebe sama a argument se
REM do promenne nikdy nedostane.
set "NOPAUSE="
:parse_args
if "%~1"=="" goto args_done
if /i "%~1"=="/nopause" set "NOPAUSE=1"
if /i "%~1"=="--nopause" set "NOPAUSE=1"
shift
goto parse_args
:args_done

if not exist "package.json" (
    echo CHYBA: v "%CD%" neni package.json.
    goto :fail
)

if not defined VULKAN_SDK (
    echo CHYBA: promenna VULKAN_SDK neni nastavena.
    echo Nainstaluj Vulkan SDK z https://vulkan.lunarg.com/sdk/home
    echo a otevri novy terminal ^(promenne se nacitaji pri otevreni okna^).
    goto :fail
)

REM --- CUDA Toolkit: vzdy nejnovejsi nainstalovana verze --------------
REM Zdedena promenna CUDA_PATH se ZAMERNE ignoruje. Instalator CUDY ji
REM nastavuje na verzi, kterou zrovna instaloval, takze klidne ukazuje na
REM starsi rady - a ta pak odmitne novejsi MSVC hlaskou "unsupported
REM Visual Studio version" a build spadne uprostred kompilace llama.cpp.
REM Konkretni toolkit jde vynutit pres WEAVE_CUDA_PATH.
set "CUDA_ROOT=%ProgramFiles%\NVIDIA GPU Computing Toolkit\CUDA"
set "CUDA_PICKED="
if defined WEAVE_CUDA_PATH (
    set "CUDA_PICKED=%WEAVE_CUDA_PATH%"
) else (
    REM /o-n = razeni podle jmena sestupne, takze v13.2 predbehne v12.6
    for /f "delims=" %%D in ('dir /b /ad /o-n "%CUDA_ROOT%" 2^>nul') do (
        if not defined CUDA_PICKED set "CUDA_PICKED=%CUDA_ROOT%\%%D"
    )
)
if not defined CUDA_PICKED (
    echo CHYBA: CUDA Toolkit nenalezen v "%CUDA_ROOT%".
    echo Stahni ho z https://developer.nvidia.com/cuda-downloads,
    echo nebo nastav WEAVE_CUDA_PATH na konkretni toolkit.
    goto :fail
)
if defined CUDA_PATH if /i not "%CUDA_PATH%"=="%CUDA_PICKED%" (
    echo POZNAMKA: systemova CUDA_PATH ukazuje na "%CUDA_PATH%",
    echo           ale stavi se s "%CUDA_PICKED%".
)
set "CUDA_PATH=%CUDA_PICKED%"
set "CUDACXX=%CUDA_PATH%\bin\nvcc.exe"
if not exist "%CUDACXX%" (
    echo CHYBA: nvcc.exe nenalezen v "%CUDA_PATH%\bin".
    goto :fail
)
set "PATH=%CUDA_PATH%\bin;%PATH%"

REM RTX 30xx=86, 40xx=89, 50xx=120. Slabsi rady maji min nez 12 GB VRAM,
REM takze jim instalator CUDA vetev stejne nenabidne a nemusi se stavet.
if not defined CMAKE_CUDA_ARCHITECTURES set "CMAKE_CUDA_ARCHITECTURES=86;89;120"

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
    goto :fail
)
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
set "PAYLOAD=src-tauri\payload\cuda"

where pnpm >nul 2>&1
if errorlevel 1 (
    echo CHYBA: 'pnpm' nebyl nalezen v PATH.
    echo Zavri vsechny terminaly a otevri novy ^(PATH se nacita pri otevreni okna^).
    goto :fail
)

echo.
echo === Weave - instalator s vyberem GPU backendu ===
echo CUDA_PATH   = %CUDA_PATH%
echo CUDA archs  = %CMAKE_CUDA_ARCHITECTURES%
echo VULKAN_SDK  = %VULKAN_SDK%
echo GENERATOR   = %CMAKE_GENERATOR%
echo.

REM --- 1/3 CUDA binarka do payloadu -------------------------------------
echo [1/3] Build CUDA verze (llama.cpp s CUDA kernely - nejdelsi krok)...
call pnpm build
if errorlevel 1 goto :buildfail
cargo build --release -p weave-app --features llm-cuda
if errorlevel 1 goto :buildfail

REM Payload se plni znovu pri kazdem buildu. Stare DLL se musi smazat:
REM po prechodu na jinou verzi CUDY by tu jinak zustaly obe sady (12.x
REM i 13.x) a instalator by narostl o pul giga mrtveho kodu, ktery navic
REM hook nakopiruje uzivateli na disk.
if exist "%PAYLOAD%" rmdir /s /q "%PAYLOAD%"
mkdir "%PAYLOAD%"
copy /y "target\release\weave-app.exe" "%PAYLOAD%\weave-app.exe" >nul
if errorlevel 1 goto :buildfail

REM cuBLAS a cudart musi jit s aplikaci - jsou soucasti Toolkitu, ne ovladace,
REM takze na cizim stroji nejsou. CUDA 13 je ma v bin\x64, starsi v bin.
set "CUDA_DLL_DIR=%CUDA_PATH%\bin\x64"
if not exist "%CUDA_DLL_DIR%\cudart64_*.dll" set "CUDA_DLL_DIR=%CUDA_PATH%\bin"
copy /y "%CUDA_DLL_DIR%\cudart64_*.dll"   "%PAYLOAD%\" >nul
copy /y "%CUDA_DLL_DIR%\cublas64_*.dll"   "%PAYLOAD%\" >nul
copy /y "%CUDA_DLL_DIR%\cublasLt64_*.dll" "%PAYLOAD%\" >nul
if errorlevel 1 (
    echo CHYBA: CUDA runtime DLL se nepodarilo zkopirovat z "%CUDA_DLL_DIR%".
    goto :fail
)

REM --- 2/3 Vulkan binarka + zabaleni -----------------------------------
echo.
echo [2/3] Build Vulkan verze a baleni instalatoru...
set "OVERRIDE=%TEMP%\weave-installer-override.json"
REM installerHooks je uz v tauri.conf.json (hook si sam pozna, ze CUDA vetev
REM v balicku neni, a nedela nic). Tady se pridavaji jen soubory payloadu.
> "%OVERRIDE%" echo {"bundle":{"createUpdaterArtifacts":false,"resources":{"payload/cuda/*":"cuda/"}}}

call pnpm tauri build --bundles nsis --features llm-vulkan --config "%OVERRIDE%"
if errorlevel 1 goto :buildfail

REM --- 3/3 Hotovo -------------------------------------------------------
REM --- Pojistka: hlavni binarka MUSI byt ta Vulkan ---
REM Kroky 1 a 2 sdileji target\release\weave-app.exe. Kdyz krok 2 selze nebo
REM se mezitim spusti rucni CUDA build, zustane tam CUDA verze a instalator by
REM ji zabalil jako hlavni (Vulkan) vetev. Na stroji bez NVIDIE by aplikace
REM neslo spustit vubec - cuBLAS DLL se u Vulkan vetve mazou.
findstr /m /c:"cublas64_" "target\release\weave-app.exe" >nul 2>&1
if not errorlevel 1 (
    echo.
    echo CHYBA: hlavni binarka target\release\weave-app.exe odkazuje na cuBLAS,
    echo tedy je to CUDA build, ne Vulkan. Instalator by ji zabalil jako Vulkan
    echo vetev a na strojich bez NVIDIE by aplikace nenabehla.
    echo Spust build znovu, at se krok 2 dokonci.
    goto :fail
)

echo.
echo [3/3] Hotovo. Instalator najdes zde:
for %%f in (target\release\bundle\nsis\*-setup.exe) do echo   %%f  (%%~zf bajtu)
echo.
echo Pri instalaci se podle nalezene karty vybere CUDA nebo Vulkan.
echo.
call :pauza
endlocal
exit /b 0

:buildfail
set "EXITCODE=%errorlevel%"
echo.
echo === Build selhal ^(kod %EXITCODE%^) - viz vypis vyse ===
call :pauza
endlocal & exit /b %EXITCODE%

:fail
call :pauza
endlocal
exit /b 1


REM Ceka na klavesu, pokud nebyl zadan /nopause.
:pauza
if not defined NOPAUSE pause
goto :eof