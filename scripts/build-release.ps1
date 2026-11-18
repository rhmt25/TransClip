# Builds the release artifacts: dist/TransClip-<ver>-windows-x64.exe, dist/TransClip-<ver>.apk
# and dist/SHA256SUMS.txt. Runs the full test suites first; any failure stops the script.
#   .\scripts\build-release.ps1            (tests + build)
#   .\scripts\build-release.ps1 -SkipTests (build only)
param([switch]$SkipTests)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$env:Path += ";$env:USERPROFILE\.cargo\bin"

function Invoke-Step([string]$what, [scriptblock]$block) {
    Write-Host "==> $what"
    & $block
    if ($LASTEXITCODE -ne 0) { throw "$what failed (exit $LASTEXITCODE)" }
}

$version = (Select-String -Path "$root\windows\Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "TransClip $version"

Push-Location "$root\windows"
try {
    if (-not $SkipTests) {
        Invoke-Step 'cargo fmt'    { cargo fmt --all -- --check }
        Invoke-Step 'cargo clippy' { cargo clippy --workspace --all-targets -- -D warnings }
        Invoke-Step 'cargo test'   { cargo test --workspace }
    }
    Invoke-Step 'cargo build --release' { cargo build --release -p transclip --locked }
} finally { Pop-Location }

Push-Location "$root\android"
try {
    if (-not $SkipTests) {
        Invoke-Step 'gradle tests' { .\gradlew.bat :proto:test :engine:test :app:testDebugUnitTest :app:lintDebug }
    }
    Invoke-Step 'gradle assembleRelease' { .\gradlew.bat :app:assembleRelease }
} finally { Pop-Location }

$dist = "$root\dist"
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item "$root\windows\target\release\transclip.exe" "$dist\TransClip-$version-windows-x64.exe" -Force

$apkDir = "$root\android\app\build\outputs\apk\release"
$apk = Get-ChildItem $apkDir -Filter '*.apk' | Select-Object -First 1
if (-not $apk) { throw "no APK found in $apkDir" }
Copy-Item $apk.FullName "$dist\TransClip-$version.apk" -Force
if ($apk.Name -like '*unsigned*') {
    Write-Warning 'The APK is unsigned: create android\keystore.properties (see docs\release.md) to sign it.'
}

Get-ChildItem $dist -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | ForEach-Object {
    $h = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
    "$h  $($_.Name)"
} | Set-Content "$dist\SHA256SUMS.txt" -Encoding ascii
Write-Host "Done. Artifacts are in $dist"
Get-Content "$dist\SHA256SUMS.txt"
