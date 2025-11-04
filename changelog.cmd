@echo off
REM Generate full CHANGELOG.md from git history
REM This regenerates the entire changelog from scratch

echo ========================================
echo Generating CHANGELOG.md
echo ========================================
echo.

git-cliff -o CHANGELOG.md

if %ERRORLEVEL% NEQ 0 (
    echo Error: git-cliff failed
    exit /b 1
)

echo.
echo ========================================
echo CHANGELOG.md updated successfully!
echo ========================================
