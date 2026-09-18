@echo off
setlocal

set "DRIVER_DIR=%~dp0third_party\UnityCapture\Install"
if not exist "%DRIVER_DIR%\UnityCaptureFilter64.dll" (
    echo UnityCapture driver files are missing from:
    echo   %DRIVER_DIR%
    exit /b 1
)

net session >nul 2>&1
if not "%errorlevel%"=="0" (
    echo Requesting administrator privileges...
    powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b 0
)

set "REGSVR32_64=%SystemRoot%\System32\regsvr32.exe"
set "REGSVR32_32=%SystemRoot%\SysWOW64\regsvr32.exe"

if exist "%DRIVER_DIR%\UnityCaptureFilter32.dll" "%REGSVR32_32%" /s "%DRIVER_DIR%\UnityCaptureFilter32.dll"
"%REGSVR32_64%" /s "%DRIVER_DIR%\UnityCaptureFilter64.dll"

if errorlevel 1 (
    echo Failed to register Unity Video Capture.
    exit /b 1
)

echo Unity Video Capture installed successfully.
echo Restart camera applications before selecting it.
exit /b 0
