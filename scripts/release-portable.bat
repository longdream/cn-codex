@echo off
setlocal EnableExtensions EnableDelayedExpansion

set "SCRIPT_DIR=%~dp0"
set "PS_SCRIPT=%SCRIPT_DIR%release-portable.ps1"

if not exist "%PS_SCRIPT%" (
  echo [release] missing script: "%PS_SCRIPT%"
  exit /b 1
)

set "PS_ARGS="

:parse
if "%~1"=="" goto run

if /I "%~1"=="--help" goto usage
if /I "%~1"=="-h" goto usage

if /I "%~1"=="--dry-run" (
  set "PS_ARGS=!PS_ARGS! -DryRun"
  shift
  goto parse
)

if /I "%~1"=="--skip-build" (
  set "PS_ARGS=!PS_ARGS! -SkipBuild"
  shift
  goto parse
)

if /I "%~1"=="--fixed" (
  set "PS_ARGS=!PS_ARGS! -Mode ""fixed"""
  shift
  goto parse
)

if /I "%~1"=="--normal" (
  set "PS_ARGS=!PS_ARGS! -Mode ""normal"""
  shift
  goto parse
)

if /I "%~1"=="--mode" (
  if "%~2"=="" goto missingValue
  if /I not "%~2"=="normal" if /I not "%~2"=="fixed" (
    echo [release] invalid mode: %~2 ^(expected normal or fixed^)
    goto usageError
  )
  set "PS_ARGS=!PS_ARGS! -Mode ""%~2"""
  shift
  shift
  goto parse
)

if /I "%~1"=="--runtime-cab-url" (
  if "%~2"=="" goto missingValue
  set "PS_ARGS=!PS_ARGS! -RuntimeCabUrl ""%~2"""
  shift
  shift
  goto parse
)

if /I "%~1"=="--runtime-cab-path" (
  if "%~2"=="" goto missingValue
  set "PS_ARGS=!PS_ARGS! -RuntimeCabPath ""%~2"""
  shift
  shift
  goto parse
)

if /I "%~1"=="--runtime-version-file" (
  if "%~2"=="" goto missingValue
  set "PS_ARGS=!PS_ARGS! -RuntimeVersionFile ""%~2"""
  shift
  shift
  goto parse
)

if /I "%~1"=="--runtime-url-file" (
  if "%~2"=="" goto missingValue
  set "PS_ARGS=!PS_ARGS! -RuntimeUrlFile ""%~2"""
  shift
  shift
  goto parse
)

if /I "%~1"=="--publish-dir" (
  if "%~2"=="" goto missingValue
  set "PS_ARGS=!PS_ARGS! -PublishDir ""%~2"""
  shift
  shift
  goto parse
)

if /I "%~1"=="--artifact-prefix" (
  if "%~2"=="" goto missingValue
  set "PS_ARGS=!PS_ARGS! -ArtifactPrefix ""%~2"""
  shift
  shift
  goto parse
)

set "PS_ARGS=!PS_ARGS! %~1"
shift
goto parse

:run
echo [release] launching portable release workflow...
powershell -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%" !PS_ARGS!
set "EXIT_CODE=%ERRORLEVEL%"
if not "%EXIT_CODE%"=="0" (
  echo [release] failed with exit code %EXIT_CODE%.
  exit /b %EXIT_CODE%
)
echo [release] done.
exit /b 0

:missingValue
echo [release] missing value for option %~1
goto usageError

:usage
echo Usage:
echo   scripts\release-portable.bat [options]
echo.
echo Options:
echo   --mode ^<normal^|fixed^>
echo   --fixed
echo   --normal
echo   --dry-run
echo   --skip-build
echo   --runtime-cab-path ^<path^>
echo   --runtime-cab-url ^<url^>
echo   --runtime-version-file ^<path^>
echo   --runtime-url-file ^<path^>
echo   --publish-dir ^<path^>
echo   --artifact-prefix ^<name^>
echo.
echo Defaults:
echo   - Default mode is normal: build small portable package without fixed WebView2 runtime.
echo   - fixed mode: package embedded fixed WebView2 runtime into the ZIP.
echo   - In fixed mode runtime source priority:
echo     1^) If release\Microsoft.WebView2.FixedVersionRuntime.^<version^>.x64.cab exists, use it.
echo     2^) Else read URL from release\webview2-runtime.url.
echo     3^) Else download WebView2.Runtime.X64.^<version^>.nupkg from NuGet.
echo     4^) If NuGet is unreachable, fallback to local installed WebView2 runtime.
echo   - Then build ^(--no-bundle^) and produce a portable ZIP.
exit /b 0

:usageError
call :usage
exit /b 2
