@echo off
chcp 65001 >nul 2>&1
setlocal EnableExtensions EnableDelayedExpansion

echo ============================================
echo   CN-Codex Build Script
echo ============================================
echo.

cd /d "%~dp0"
set "PROJECT_DIR=%~dp0"
set "PROJECT_DIR=%PROJECT_DIR:~0,-1%"

if not exist "logs" mkdir logs
for /f %%i in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd_HHmmss"') do set "BUILD_TS=%%i"
if not defined BUILD_TS set "BUILD_TS=fallback_%RANDOM%"
set "LOGFILE=logs\build_%BUILD_TS%.log"

echo [%date% %time%] === Build started === > "%LOGFILE%"

REM Optional public update server base URL
if not defined UPDATE_BASE_URL set "UPDATE_BASE_URL=http://47.113.221.244:5005"
if not defined UPDATE_NOTES set "UPDATE_NOTES="

REM Cargo network resilience for flaky registry mirrors
if not defined CARGO_HTTP_TIMEOUT set "CARGO_HTTP_TIMEOUT=180"
if not defined CARGO_NET_RETRY set "CARGO_NET_RETRY=8"

REM Check codex availability
where codex.exe >nul 2>&1
if errorlevel 1 (
    if defined CN_CODEX_EXE (
        echo [INFO] Using CN_CODEX_EXE=%CN_CODEX_EXE%
        echo [%date% %time%] Using CN_CODEX_EXE=%CN_CODEX_EXE% >> "%LOGFILE%"
    ) else if defined CODEX_CLI_PATH (
        echo [INFO] Using CODEX_CLI_PATH=%CODEX_CLI_PATH%
        echo [%date% %time%] Using CODEX_CLI_PATH=%CODEX_CLI_PATH% >> "%LOGFILE%"
    ) else (
        echo [WARN] codex.exe not found in PATH.
        echo [WARN] Set CN_CODEX_EXE or CODEX_CLI_PATH for runtime.
        echo [%date% %time%] WARN: codex.exe not in PATH >> "%LOGFILE%"
    )
) else (
    echo [INFO] codex.exe found in PATH.
    echo [%date% %time%] codex.exe found in PATH >> "%LOGFILE%"
)

echo.
if defined TAURI_BUNDLES (
    set "TAURI_BUILD_ARGS=--bundles %TAURI_BUNDLES%"
    echo [INFO] Bundle targets: %TAURI_BUNDLES%
    echo [%date% %time%] Bundle targets=%TAURI_BUNDLES% >> "%LOGFILE%"
) else (
    set "TAURI_BUILD_ARGS=--no-bundle"
    if not defined CARGO_PROFILE_RELEASE_LTO set "CARGO_PROFILE_RELEASE_LTO=false"
    if not defined CARGO_PROFILE_RELEASE_CODEGEN_UNITS set "CARGO_PROFILE_RELEASE_CODEGEN_UNITS=8"
    if not defined CARGO_INCREMENTAL set "CARGO_INCREMENTAL=0"
    echo [INFO] Bundle targets: none ^(fast mode^).
    echo [INFO] Set TAURI_BUNDLES=nsis or msi if installer packages are needed.
    echo [INFO] Fast release profile defaults: lto=false, codegen-units=8, incremental=off ^(override via env^).
    echo [%date% %time%] Bundle targets=none ^(fast mode^) >> "%LOGFILE%"
    echo [%date% %time%] Fast release profile defaults: lto=false, codegen-units=8, incremental=off >> "%LOGFILE%"
)

REM Always disable incremental for this scripted release path.
if not defined CARGO_INCREMENTAL set "CARGO_INCREMENTAL=0"
if not defined CARGO_PROFILE_RELEASE_LTO set "CARGO_PROFILE_RELEASE_LTO=false"
if not defined CARGO_PROFILE_RELEASE_CODEGEN_UNITS set "CARGO_PROFILE_RELEASE_CODEGEN_UNITS=8"
REM Keep the release build within the memory budget of typical 16 GB Windows machines.
REM Override CARGO_BUILD_JOBS when building on a larger machine.
if not defined CARGO_BUILD_JOBS set "CARGO_BUILD_JOBS=1"
echo [INFO] Cargo build jobs: %CARGO_BUILD_JOBS%
echo [%date% %time%] Cargo build jobs=%CARGO_BUILD_JOBS% >> "%LOGFILE%"

echo.
echo [0/5] Checking disk space and cleaning bulky Rust cache...
echo [%date% %time%] [0/5] Disk check + rust cache cleanup >> "%LOGFILE%"
set "PROJECT_DRIVE=?"
set "PROJECT_FREE_GB=0"
for /f "usebackq tokens=1,2 delims=|" %%A in (`powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_DIR%\scripts\check-disk-space.ps1" -Path "%PROJECT_DIR%"`) do (
    set "PROJECT_DRIVE=%%A"
    set "PROJECT_FREE_GB=%%B"
)
echo [INFO] Project drive %PROJECT_DRIVE% free=%PROJECT_FREE_GB% GB
echo [%date% %time%] Project drive %PROJECT_DRIVE% free=%PROJECT_FREE_GB% GB >> "%LOGFILE%"

REM Prefer soft clean by default; escalate when disk is tight.
set "CLEAN_MODE=auto"
if /I "%~1"=="fullclean" set "CLEAN_MODE=full"
if /I "%~1"=="clean" set "CLEAN_MODE=debug"
echo [INFO] Cleaning Rust cache ^(mode=%CLEAN_MODE%^)... this may take a while
powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_DIR%\scripts\clean-rust-cache.ps1" -Mode %CLEAN_MODE% >> "%LOGFILE%" 2>&1
if errorlevel 1 (
    echo [WARN] Rust cache cleanup reported errors; continuing build.
    echo [%date% %time%] WARN: rust cache cleanup failed >> "%LOGFILE%"
) else (
    echo       OK: rust cache cleaned ^(mode=%CLEAN_MODE%^)
)

echo.
echo [1/5] Building Tauri app ^(includes frontend build, bins: cn-codex + updater^)...
echo [%date% %time%] [1/5] Building Tauri app >> "%LOGFILE%"
call pnpm tauri build %TAURI_BUILD_ARGS% >> "%LOGFILE%" 2>&1
if errorlevel 1 (
    echo [ERROR] Tauri build failed! See %LOGFILE%
    echo [HINT] Fast mode uses --no-bundle. Use TAURI_BUNDLES only when you need installers.
    echo [HINT] If error is disk full ^(os error 112^), run: build.bat fullclean
    echo [%date% %time%] ERROR: Tauri build failed ^(args=%TAURI_BUILD_ARGS%^) >> "%LOGFILE%"
    pause
    exit /b 1
)
echo       OK

set "RELEASE_DIR=%PROJECT_DIR%\src-tauri\target\release"
set "MAIN_EXE=%RELEASE_DIR%\cn-codex.exe"
set "UPDATER_EXE=%RELEASE_DIR%\updater.exe"

if not exist "%MAIN_EXE%" (
    echo [ERROR] Main exe not found: %MAIN_EXE%
    echo [%date% %time%] ERROR: main exe missing >> "%LOGFILE%"
    pause
    exit /b 1
)

if not exist "%UPDATER_EXE%" (
    echo [WARN] updater.exe not produced by tauri build, building updater bin explicitly...
    echo [%date% %time%] WARN: building updater bin explicitly >> "%LOGFILE%"
    call cargo build --release --manifest-path "%PROJECT_DIR%\src-tauri\Cargo.toml" --bin updater >> "%LOGFILE%" 2>&1
    if errorlevel 1 (
        echo [ERROR] Failed to build updater.exe. See %LOGFILE%
        echo [%date% %time%] ERROR: cargo build --bin updater failed >> "%LOGFILE%"
        pause
        exit /b 1
    )
)

if not exist "%UPDATER_EXE%" (
    echo [ERROR] updater.exe not found after build: %UPDATER_EXE%
    echo [%date% %time%] ERROR: updater.exe missing >> "%LOGFILE%"
    pause
    exit /b 1
)

echo.
echo [2/5] Reading app version...
echo [%date% %time%] [2/5] Reading version >> "%LOGFILE%"
for /f "usebackq delims=" %%v in (`powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_DIR%\scripts\read-app-version.ps1"`) do set "APP_VERSION=%%v"
if not defined APP_VERSION (
    echo [ERROR] Failed to resolve app version
    echo [%date% %time%] ERROR: version resolve failed >> "%LOGFILE%"
    pause
    exit /b 1
)
echo       version = %APP_VERSION%
echo [%date% %time%] version=%APP_VERSION% >> "%LOGFILE%"

echo.
echo [3/5] Copying runtime artifacts to build\...
echo [%date% %time%] [3/5] Copying artifacts >> "%LOGFILE%"
if not exist "build" mkdir build
copy /Y "%MAIN_EXE%" "build\CN-Codex.exe" >nul
copy /Y "%MAIN_EXE%" "build\cn-codex.exe" >nul
copy /Y "%UPDATER_EXE%" "build\updater.exe" >nul
if defined TAURI_BUNDLES (
    if exist "src-tauri\target\release\bundle\nsis\*.exe" copy /Y src-tauri\target\release\bundle\nsis\*.exe build\ >nul 2>&1
    if exist "src-tauri\target\release\bundle\msi\*.msi" copy /Y src-tauri\target\release\bundle\msi\*.msi build\ >nul 2>&1
)
echo       OK: build\CN-Codex.exe + build\updater.exe

echo.
echo [4/5] Preparing update-server upload package...
echo [%date% %time%] [4/5] Preparing update artifacts >> "%LOGFILE%"
set "ARTIFACTS_DIR=%PROJECT_DIR%\build\update-artifacts"
if exist "%ARTIFACTS_DIR%" rmdir /s /q "%ARTIFACTS_DIR%"
mkdir "%ARTIFACTS_DIR%"

powershell -NoProfile -ExecutionPolicy Bypass -File "%PROJECT_DIR%\scripts\prepare-update-artifacts.ps1" ^
  -Version "%APP_VERSION%" ^
  -MainExe "%MAIN_EXE%" ^
  -UpdaterExe "%UPDATER_EXE%" ^
  -OutDir "%ARTIFACTS_DIR%" ^
  -BaseUrl "%UPDATE_BASE_URL%" ^
  -Notes "%UPDATE_NOTES%" >> "%LOGFILE%" 2>&1
if errorlevel 1 (
    echo [ERROR] Failed to prepare update artifacts. See %LOGFILE%
    echo [%date% %time%] ERROR: prepare-update-artifacts failed >> "%LOGFILE%"
    pause
    exit /b 1
)

REM Also keep a copy of latest.json next to build outputs for convenience.
copy /Y "%ARTIFACTS_DIR%\latest.json" "build\latest.json" >nul
echo       OK

echo.
echo [5/5] Summary ^(no auto-launch so artifacts stay stable^)...
echo [%date% %time%] [5/5] Summary >> "%LOGFILE%"
echo [%date% %time%] === Build complete === >> "%LOGFILE%"

echo.
echo ============================================
echo   Build complete!
echo.
echo   Client package ^(2 exes^):
echo     build\CN-Codex.exe
echo     build\updater.exe
echo     build\latest.json
echo.
echo   Server overwrite package:
echo     build\update-artifacts\update-upload\latest.json
echo     build\update-artifacts\update-upload\files\CN-Codex-%APP_VERSION%.zip
echo.
echo   Upload to server ^(example^):
echo     scp -r build\update-artifacts\update-upload\* root@47.113.221.244:/opt/cn-codex-update/public/
echo.
echo   Log:  %LOGFILE%
echo ============================================
echo.
echo build\ :
dir /b "build\*.exe" "build\latest.json" 2>nul
echo.
echo update-upload\ :
dir /b "build\update-artifacts\update-upload" 2>nul
dir /b "build\update-artifacts\update-upload\files" 2>nul
echo.
pause
