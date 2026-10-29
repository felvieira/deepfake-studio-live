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

rem Com placa NVIDIA, as bibliotecas CUDA 12 que o onnxruntime-gpu precisa.
rem Sem elas o app abre normalmente, mas roda na CPU, bem mais devagar.
where nvidia-smi >nul 2>&1
if not errorlevel 1 (
    echo Installing NVIDIA CUDA libraries for GPU acceleration...
    "venv\Scripts\python.exe" -m pip install nvidia-cuda-runtime-cu12 nvidia-cublas-cu12 nvidia-cudnn-cu12 nvidia-cufft-cu12 nvidia-curand-cu12 nvidia-cuda-nvrtc-cu12
    if errorlevel 1 exit /b 1
)

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
