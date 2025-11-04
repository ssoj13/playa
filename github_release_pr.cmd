@echo off
:: Create a Pull Request from dev to main for release
::
:: Usage:
::   create_release_pr.cmd [version]
::
:: Examples:
::   create_release_pr.cmd v0.2.0
::   create_release_pr.cmd         (will prompt for version)

setlocal

:: Get version from argument or prompt
set VERSION=%1
if "%VERSION%"=="" (
    set /p VERSION="Enter release version (e.g., v0.2.0): "
)

:: Remove 'v' prefix if present, then add it back for consistency
set VERSION=%VERSION:v=%
set VERSION=v%VERSION%

:: Get commit count between main and dev
echo.
echo Calculating changes between main and dev...
for /f %%i in ('git rev-list --count origin/main..dev') do set COMMIT_COUNT=%%i

:: Create PR title and body
set TITLE=Release %VERSION%
set BODY=Release %VERSION% - %COMMIT_COUNT% commits from dev branch

echo.
echo Creating Pull Request:
echo   From: dev
echo   To:   main
echo   Title: %TITLE%
echo   Commits: %COMMIT_COUNT%
echo.

:: Create the PR
gh pr create --base main --head dev --title "%TITLE%" --body "%BODY%"

if errorlevel 1 (
    echo.
    echo Error: Failed to create pull request
    echo Make sure you have:
    echo   - Pushed your dev branch to origin
    echo   - Authenticated with 'gh auth login'
    exit /b 1
)

echo.
echo ✓ Pull Request created successfully!
echo.
echo Next steps:
echo   1. Review the PR on GitHub
echo   2. Merge when ready: gh pr merge --merge
echo   3. Create tag on main: git tag %VERSION% ^&^& git push origin %VERSION%
echo   4. Release workflow will create GitHub Release automatically

endlocal
