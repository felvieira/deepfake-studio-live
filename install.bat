@echo off
setlocal EnableExtensions
cd /d "%~dp0"

echo [1/3] Preparing Python environment...
where py >nul 2>&1
if errorlevel 1 (
    echo Python launcher ^(py^) was not found. Install Python 3.11-3.13 and retry.
    exit /b 1
)

if not exist "venv\Scripts\python.exe" (
    py -3 -m venv venv
    if errorlevel 1 exit /b 1
)

echo [2/3] Installing Deep-Live-Cam dependencies...
"venv\Scripts\python.exe" -m pip install --upgrade pip
if errorlevel 1 exit /b 1
"venv\Scripts\python.exe" -m pip install -r requirements.txt
if errorlevel 1 exit /b 1

echo [3/3] Installing the Unity Video Capture device...
call "%~dp0install_virtual_camera.bat"
if errorlevel 1 (
    echo The Python environment is ready, but the virtual camera installation failed.
    exit /b 1
)

echo.
echo Installation complete.
echo Start the application with: venv\Scripts\python.exe run.py
echo Enable Virtual Camera in the Camera panel when needed.
exit /b 0
