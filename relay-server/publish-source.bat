@echo off
chcp 65001 >nul
setlocal

echo ============================================
echo    CN-Codex Relay - Pack for Server Deploy
echo ============================================
echo.

set "PROJECT_DIR=%~dp0"
set "PUBLISH_DIR=%PROJECT_DIR%publish"

:: [1] 清理
echo [1/4] Cleaning publish folder...
if exist "%PUBLISH_DIR%" rd /s /q "%PUBLISH_DIR%"
mkdir "%PUBLISH_DIR%\relay-server\src"

:: [2] 复制 Rust 源码
echo [2/4] Packing relay-server source...
xcopy "%PROJECT_DIR%src" "%PUBLISH_DIR%\relay-server\src\" /E /I /Q >nul
copy "%PROJECT_DIR%Cargo.toml" "%PUBLISH_DIR%\relay-server\Cargo.toml" >nul

:: [3] 复制 mobile-dist
echo [3/4] Copying mobile-dist...
set "MOBILE_DIST=%PROJECT_DIR%..\mobile-dist"
if exist "%MOBILE_DIST%" (
    xcopy "%MOBILE_DIST%" "%PUBLISH_DIR%\mobile-dist\" /E /I /Q >nul
    echo   - mobile-dist copied.
) else (
    echo   [WARN] mobile-dist not found.
    echo          Run: cd ..\mobile-web ^& pnpm build
    mkdir "%PUBLISH_DIR%\mobile-dist"
)

:: [4] 复制部署脚本并确保 LF 换行
echo [4/4] Copying deploy scripts...
copy "%PROJECT_DIR%deploy\setup.sh" "%PUBLISH_DIR%\setup.sh" >nul

:: 转换为 Unix 换行符（LF）
powershell -Command "$f='%PUBLISH_DIR%\setup.sh'; $utf8=[System.Text.UTF8Encoding]::new($false); [System.IO.File]::WriteAllText($f, [System.IO.File]::ReadAllText($f).Replace(\"`r`n\",\"`n\"), $utf8)"

echo.
echo ============================================
echo   Output: %PUBLISH_DIR%\
echo.
echo   Contents:
echo     relay-server/     Rust source code
echo     mobile-dist/      SPA static files
echo     setup.sh          One-click setup script
echo.
echo   Deploy:
echo     1. Upload publish/ to server:
echo        scp -r publish/* root@server:/tmp/relay/
echo.
echo     2. SSH and run:
echo        cd /tmp/relay
echo        chmod +x setup.sh
echo        ./setup.sh
echo.
echo   setup.sh will automatically:
echo     - Install Rust (if needed)
echo     - Build the binary
echo     - Install as systemd service
echo     - Start on port 8080
echo     - Open firewall port
echo ============================================
pause
