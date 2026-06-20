@echo off
setlocal enabledelayedexpansion

echo ============================================
echo    CN-Codex Release Build
echo ============================================
echo.

set "PROJECT_DIR=%~dp0"
set "PUBLISH_DIR=%PROJECT_DIR%publish"
set "CODEY_SRC=%PROJECT_DIR%codey"
set "ICONS_DIR=%PROJECT_DIR%src-tauri\icons"
set "FORCE_FULL_REBUILD=0"
if /I "%~1"=="clean" set "FORCE_FULL_REBUILD=1"

:: ---------------------------------------------------------------------------
:: Release build hardening:
:: 1) Force-disable Cargo incremental for deterministic release artifacts.
:: 2) Clear stale Rust/Cargo env overrides from parent terminal sessions.
::    (A previous temporary env like CARGO_TARGET_DIR / RUSTFLAGS can poison
::     release builds and trigger metadata-stub errors on Windows.)
:: 3) Use a low-memory release override for Windows build stability.
::    This prevents "metadata stub", E0786, and rustc OOM/pagefile failures.
:: 4) Publish as portable green edition only (no NSIS/MSI installer).
::    This keeps release speed high and avoids installer download overhead.
:: ---------------------------------------------------------------------------
set "CARGO_INCREMENTAL=0"
set "RUSTFLAGS="
set "CARGO_ENCODED_RUSTFLAGS="
set "CARGO_TARGET_DIR="
set "RUSTC_WRAPPER="
set "CARGO_BUILD_JOBS=1"
set "CARGO_PROFILE_RELEASE_LTO=false"
set "CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16"
set "CARGO_PROFILE_RELEASE_OPT_LEVEL=2"

:: Check prerequisites
where pnpm >nul 2>&1
if %errorlevel% neq 0 (
    echo [ERROR] pnpm not found. Please install pnpm first.
    pause
    exit /b 1
)

where cargo >nul 2>&1
if %errorlevel% neq 0 (
    echo [ERROR] cargo not found. Please install Rust first.
    pause
    exit /b 1
)

:: Check icon files
if not exist "%ICONS_DIR%\icon.ico" (
    echo [ERROR] icon.ico not found in src-tauri/icons/
    echo   Please run: pnpm tauri icon [source-image.png]
    pause
    exit /b 1
)
if not exist "%ICONS_DIR%\32x32.png" (
    echo [ERROR] 32x32.png not found in src-tauri/icons/
    pause
    exit /b 1
)
if not exist "%ICONS_DIR%\128x128.png" (
    echo [ERROR] 128x128.png not found in src-tauri/icons/
    pause
    exit /b 1
)

:: Clean previous publish folder
echo [1/7] Cleaning previous publish folder...
if exist "%PUBLISH_DIR%" (
    rmdir /s /q "%PUBLISH_DIR%"
)
mkdir "%PUBLISH_DIR%"

:: Install frontend dependencies only when node_modules is missing.
:: This avoids unnecessary dependency resolution on every publish run.
echo [2/7] Checking frontend dependencies...
cd /d "%PROJECT_DIR%"
if not exist "%PROJECT_DIR%node_modules" (
    echo   - node_modules not found, running pnpm install...
    call pnpm install --frozen-lockfile
    if %errorlevel% neq 0 (
        call pnpm install
        if %errorlevel% neq 0 (
            echo [ERROR] pnpm install failed.
            pause
            exit /b 1
        )
    )
) else (
    echo   - node_modules exists, skip install for faster publish.
)

:: Build mobile-web
echo [3/8] Building mobile-web...
cd /d "%PROJECT_DIR%mobile-web"
if not exist "%PROJECT_DIR%mobile-web\node_modules" (
    echo   - Installing mobile-web dependencies...
    call pnpm install
    if %errorlevel% neq 0 (
        echo [ERROR] mobile-web pnpm install failed.
        pause
        exit /b 1
    )
)
call pnpm build
if %errorlevel% neq 0 (
    echo [ERROR] mobile-web build failed.
    pause
    exit /b 1
)
echo   - mobile-web built to mobile-dist/
cd /d "%PROJECT_DIR%"

:: Build Tauri app in release mode (portable only, no installer bundle)
echo [4/8] Building Tauri release (portable, no installer)...
:: For release speed, default is incremental publish without cargo clean.
:: If cache corruption is suspected, run "publish.bat clean" for full rebuild.
if "%FORCE_FULL_REBUILD%"=="1" (
    echo   - Full rebuild requested, cleaning Rust target cache...
    call cargo clean --manifest-path "%PROJECT_DIR%src-tauri\Cargo.toml"
    if %errorlevel% neq 0 (
        echo [ERROR] cargo clean failed.
        pause
        exit /b 1
    )
) else (
    echo   - Skip cargo clean for faster publish. Use "publish.bat clean" when needed.
)

:: --no-bundle means:
:: - still produces optimized release EXE in src-tauri/target/release
:: - does NOT produce installer files under bundle/nsis or bundle/msi
call pnpm tauri build --no-bundle
if %errorlevel% neq 0 (
    echo [ERROR] Tauri portable build failed.
    pause
    exit /b 1
)

:: Copy artifacts to publish folder
echo [5/8] Copying portable artifacts to publish folder...

set "RELEASE_DIR=%PROJECT_DIR%src-tauri\target\release"

:: Copy the exe directly
if exist "%RELEASE_DIR%\cn-codex.exe" (
    copy "%RELEASE_DIR%\cn-codex.exe" "%PUBLISH_DIR%\CN-Codex.exe" >nul
    echo   - CN-Codex.exe
)

:: NOTE: Installer artifacts are intentionally skipped.
::       publish/ now contains only green portable runtime files.

:: Copy runtime DLLs
for %%f in ("%RELEASE_DIR%\*.dll") do (
    copy "%%f" "%PUBLISH_DIR%\" >nul 2>&1
)

:: Copy codey/ resources (skills + plugins only)
echo [6/8] Copying codey runtime resources (skills/plugins/robots)...

set "CODEY_DEST=%PUBLISH_DIR%\codey"
mkdir "%CODEY_DEST%"

:: Copy skills
if exist "%CODEY_SRC%\skills" (
    xcopy "%CODEY_SRC%\skills" "%CODEY_DEST%\skills\" /E /I /Q /Y >nul
    echo   - codey/skills/ copied
) else (
    mkdir "%CODEY_DEST%\skills"
    echo   - codey/skills/ (empty, created)
)

:: Copy plugins
if exist "%CODEY_SRC%\plugins" (
    xcopy "%CODEY_SRC%\plugins" "%CODEY_DEST%\plugins\" /E /I /Q /Y >nul
    echo   - codey/plugins/ copied
) else (
    mkdir "%CODEY_DEST%\plugins"
    echo   - codey/plugins/ (empty, created)
)

:: Create empty runtime directories
if exist "%CODEY_SRC%\robots" (
    xcopy "%CODEY_SRC%\robots" "%CODEY_DEST%\robots\" /E /I /Q /Y >nul
    echo   - codey/robots/ copied
) else (
    mkdir "%CODEY_DEST%\robots"
    echo   - codey/robots/ (empty, created)
)
mkdir "%CODEY_DEST%\sessions" 2>nul
mkdir "%CODEY_DEST%\memories" 2>nul
mkdir "%CODEY_DEST%\browser" 2>nul
echo   - codey/sessions/ (empty, ready)
echo   - codey/memories/ (empty, ready)
echo   - codey/browser/ (empty, ready)

:: Ensure no sensitive files are included
if exist "%CODEY_DEST%\config.toml" del "%CODEY_DEST%\config.toml"
if exist "%CODEY_DEST%\usage.db" del "%CODEY_DEST%\usage.db"
if exist "%CODEY_DEST%\usage.db-wal" del "%CODEY_DEST%\usage.db-wal"
if exist "%CODEY_DEST%\usage.db-shm" del "%CODEY_DEST%\usage.db-shm"
if exist "%CODEY_DEST%\hooks.json" del "%CODEY_DEST%\hooks.json"
if exist "%CODEY_DEST%\browser\webview-data" rmdir /s /q "%CODEY_DEST%\browser\webview-data"
if exist "%CODEY_DEST%\browser\screenshots" rmdir /s /q "%CODEY_DEST%\browser\screenshots"
if exist "%CODEY_DEST%\browser\visible-browser.json" del "%CODEY_DEST%\browser\visible-browser.json"

:: Remove any residual WebView2 user data from publish dir
for /d %%d in ("%PUBLISH_DIR%\EBWebView*") do rmdir /s /q "%%d" 2>nul

:: Copy mobile-dist to publish
echo [7/8] Copying mobile-dist...
set "MOBILE_DIST_SRC=%PROJECT_DIR%mobile-dist"
set "MOBILE_DIST_DEST=%PUBLISH_DIR%\mobile-dist"
if exist "%MOBILE_DIST_SRC%" (
    xcopy "%MOBILE_DIST_SRC%" "%MOBILE_DIST_DEST%\" /E /I /Q /Y >nul
    echo   - mobile-dist/ copied
) else (
    mkdir "%MOBILE_DIST_DEST%"
    echo   - mobile-dist/ (empty, mobile-web not built)
)

:: Bundle embedded Node.js portable runtime into codey/node/
echo [8/8] Bundling embedded Node.js portable...

set "NODE_VERSION=22.16.0"
set "NODE_ARCHIVE=node-v%NODE_VERSION%-win-x64.zip"
set "NODE_URL=https://nodejs.org/dist/v%NODE_VERSION%/%NODE_ARCHIVE%"
set "NODE_CACHE_DIR=%PROJECT_DIR%tools\node-cache"
set "NODE_DEST=%CODEY_DEST%\node"

:: Use cached download if available, otherwise download
if not exist "%NODE_CACHE_DIR%" mkdir "%NODE_CACHE_DIR%"
if not exist "%NODE_CACHE_DIR%\%NODE_ARCHIVE%" (
    echo   - Downloading Node.js v%NODE_VERSION% portable...
    powershell -Command "Invoke-WebRequest -Uri '%NODE_URL%' -OutFile '%NODE_CACHE_DIR%\%NODE_ARCHIVE%'" 2>nul
    if %errorlevel% neq 0 (
        echo [WARN] Failed to download Node.js. Online tools will require user-installed Node.
        goto :skip_node
    )
    echo   - Downloaded to tools/node-cache/
) else (
    echo   - Using cached Node.js archive from tools/node-cache/
)

:: Extract to temporary directory then copy essentials
set "NODE_TMP=%CODEY_DEST%\_node_tmp"
echo   - Extracting Node.js portable...
powershell -Command "Expand-Archive -Path '%NODE_CACHE_DIR%\%NODE_ARCHIVE%' -DestinationPath '%NODE_TMP%' -Force" 2>nul
if %errorlevel% neq 0 (
    echo [WARN] Failed to extract Node.js archive.
    if exist "%NODE_TMP%" rmdir /s /q "%NODE_TMP%"
    goto :skip_node
)

:: Move essential files only (node.exe, npm, npx, node_modules/npm)
set "NODE_EXTRACTED=%NODE_TMP%\node-v%NODE_VERSION%-win-x64"
mkdir "%NODE_DEST%"
copy "%NODE_EXTRACTED%\node.exe" "%NODE_DEST%\" >nul
copy "%NODE_EXTRACTED%\npm" "%NODE_DEST%\" >nul 2>&1
copy "%NODE_EXTRACTED%\npm.cmd" "%NODE_DEST%\" >nul 2>&1
copy "%NODE_EXTRACTED%\npx" "%NODE_DEST%\" >nul 2>&1
copy "%NODE_EXTRACTED%\npx.cmd" "%NODE_DEST%\" >nul 2>&1
copy "%NODE_EXTRACTED%\corepack" "%NODE_DEST%\" >nul 2>&1
copy "%NODE_EXTRACTED%\corepack.cmd" "%NODE_DEST%\" >nul 2>&1
if exist "%NODE_EXTRACTED%\node_modules" (
    xcopy "%NODE_EXTRACTED%\node_modules" "%NODE_DEST%\node_modules\" /E /I /Q /Y >nul
)
echo   - codey/node/ bundled (Node.js v%NODE_VERSION%)

:: Cleanup temp
if exist "%NODE_TMP%" rmdir /s /q "%NODE_TMP%"

:skip_node

echo.
echo Build complete!
echo.
echo ============================================
echo   Output: %PUBLISH_DIR%
echo ============================================
echo.
echo Contents:
echo.
dir /b "%PUBLISH_DIR%"
echo.
echo codey/ resources:
dir /b "%CODEY_DEST%"
echo.
echo NOTE: Users need to configure their model provider
echo       via Settings on first launch.
echo.
echo Done! Press any key to open the publish folder.
pause >nul
explorer "%PUBLISH_DIR%"
