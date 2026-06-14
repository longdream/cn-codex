@echo off
chcp 65001 >nul
setlocal

echo ============================================
echo    CN-Codex Relay Server - Release Build
echo ============================================
echo.

set "PROJECT_DIR=%~dp0"
set "PUBLISH_DIR=%PROJECT_DIR%publish"
set "TARGET=x86_64-unknown-linux-musl"

:: [1] 清理输出目录
echo [1/5] Cleaning publish folder...
if exist "%PUBLISH_DIR%" rd /s /q "%PUBLISH_DIR%"
mkdir "%PUBLISH_DIR%"

:: [2] 检查工具链
echo [2/5] Checking build tools...

:: 优先使用 cargo-zigbuild（无需 Docker，Windows 上最佳方案）
where cargo-zigbuild >nul 2>&1
if %ERRORLEVEL% equ 0 (
    echo   Using cargo-zigbuild for cross-compilation.
    set "BUILD_CMD=cargo zigbuild --release --target %TARGET%"
    goto :do_build
)

:: 备选：检查 cross（需要 Docker）
where cross >nul 2>&1
if %ERRORLEVEL% equ 0 (
    echo   Using cross for cross-compilation.
    set "BUILD_CMD=cross build --release --target %TARGET%"
    goto :do_build
)

:: 都没有，提示安装
echo.
echo [ERROR] No cross-compilation tool found.
echo.
echo   Please install one of the following:
echo.
echo   Option 1 (Recommended): cargo-zigbuild
echo     1. Install zig: winget install zig.zig
echo     2. Install zigbuild: cargo install cargo-zigbuild
echo.
echo   Option 2: cross (requires Docker)
echo     cargo install cross
echo.
echo   Option 3: Build on Linux server directly
echo     Upload source code and run: cargo build --release
echo     Then copy target/release/cn-codex-relay to publish/
echo.
pause
exit /b 1

:do_build
:: [3] 编译
echo [3/5] Building release binary for %TARGET%...
%BUILD_CMD%
if %ERRORLEVEL% neq 0 (
    echo [ERROR] Build failed.
    pause
    exit /b 1
)

:: [4] 复制产物
echo [4/5] Copying files to publish folder...
copy "%PROJECT_DIR%target\%TARGET%\release\cn-codex-relay" "%PUBLISH_DIR%\cn-codex-relay"
if not exist "%PUBLISH_DIR%\cn-codex-relay" (
    echo [ERROR] Binary not found after build.
    pause
    exit /b 1
)

:: 复制 mobile-dist（从父项目）
set "MOBILE_DIST=%PROJECT_DIR%..\mobile-dist"
if exist "%MOBILE_DIST%" (
    xcopy "%MOBILE_DIST%" "%PUBLISH_DIR%\mobile-dist\" /E /I /Q >nul
    echo   - mobile-dist copied.
) else (
    echo [WARN] mobile-dist not found at %MOBILE_DIST%
    echo        Please build mobile-web first: cd ..\mobile-web ^& pnpm build
    mkdir "%PUBLISH_DIR%\mobile-dist"
)

:: 复制部署脚本
copy "%PROJECT_DIR%deploy\deploy.sh" "%PUBLISH_DIR%\deploy.sh" >nul
copy "%PROJECT_DIR%deploy\cn-codex-relay.service" "%PUBLISH_DIR%\cn-codex-relay.service" >nul

:: 复制服务器端编译脚本（备用）
copy "%PROJECT_DIR%deploy\build-on-server.sh" "%PUBLISH_DIR%\build-on-server.sh" >nul 2>nul

:: [5] 完成
echo [5/5] Done!
echo.
echo ============================================
echo   Output: %PUBLISH_DIR%\
echo.
echo   Files:
echo     cn-codex-relay          (Linux x86_64 binary)
echo     mobile-dist/            (SPA static files)
echo     deploy.sh               (One-click deploy script)
echo     cn-codex-relay.service  (systemd unit file)
echo.
echo   Deploy steps:
echo     1. Upload all files in publish/ to server
echo        scp -r publish/* root@your-server:/tmp/cn-codex-relay/
echo     2. SSH into server and run:
echo        cd /tmp/cn-codex-relay
echo        chmod +x deploy.sh
echo        ./deploy.sh
echo ============================================
pause
