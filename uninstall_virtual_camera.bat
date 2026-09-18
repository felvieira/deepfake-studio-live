@echo off
setlocal

set "DRIVER_DIR=%~dp0third_party\UnityCapture\Install"
net session >nul 2>&1
if not "%errorlevel%"=="0" (
    echo Requesting administrator privileges...
    powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b 0
)

set "REGSVR32_64=%SystemRoot%\System32\regsvr32.exe"
set "REGSVR32_32=%SystemRoot%\SysWOW64\regsvr32.exe"

if exist "%DRIVER_DIR%\UnityCaptureFilter32.dll" "%REGSVR32_32%" /u /s "%DRIVER_DIR%\UnityCaptureFilter32.dll"
if exist "%DRIVER_DIR%\UnityCaptureFilter64.dll" "%REGSVR32_64%" /u /s "%DRIVER_DIR%\UnityCaptureFilter64.dll"

echo Unity Video Capture removed.
exit /b 0
