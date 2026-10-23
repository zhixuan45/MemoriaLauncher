@echo off
chcp 65001 >nul
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0package.ps1"
if %ERRORLEVEL% neq 0 (
    echo.
    echo 打包失败，请检查上方错误提示。
    pause
    exit /b %ERRORLEVEL%
)
pause
