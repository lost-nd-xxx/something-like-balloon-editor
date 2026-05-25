# build_release.ps1
# リリースビルドを行い、配布用 zip を作成するスクリプト
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ProjectRoot = $PSScriptRoot
$AppName     = "balloon_editor"
$ExePath     = "$ProjectRoot\target\release\$AppName.exe"
$ZipName     = "$AppName.zip"
$ZipPath     = "$ProjectRoot\$ZipName"
$StagingDir  = "$ProjectRoot\target\release\_staging"

# ---------------------------------------------------------------------------
# 1. リリースビルド
# ---------------------------------------------------------------------------
Write-Host ">>> cargo build --release ..." -ForegroundColor Cyan
Push-Location $ProjectRoot
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release が失敗しました (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
}
Write-Host "    ビルド成功: $ExePath" -ForegroundColor Green

# ---------------------------------------------------------------------------
# 2. ステージングディレクトリを準備
# ---------------------------------------------------------------------------
Write-Host ">>> ステージングディレクトリを準備 ..." -ForegroundColor Cyan
if (Test-Path $StagingDir) { Remove-Item $StagingDir -Recurse -Force }
New-Item -ItemType Directory -Path $StagingDir | Out-Null

# ---------------------------------------------------------------------------
# 3. ファイルをステージングへコピー
# ---------------------------------------------------------------------------
$FilesToCopy = @(
    "$ExePath",
    "$ProjectRoot\README.md",
    "$ProjectRoot\MANUAL.md",
    "$ProjectRoot\LICENSE.txt",
    "$ProjectRoot\ThirdPartyNotices.txt"
)

foreach ($f in $FilesToCopy) {
    if (-not (Test-Path $f)) { throw "コピー対象が見つかりません: $f" }
    Copy-Item $f -Destination $StagingDir
    Write-Host "    コピー: $(Split-Path $f -Leaf)"
}

# resource\assets\something_like_balloon\ をフォルダごとコピー
$AssetSrc = "$ProjectRoot\resource\assets\something_like_balloon"
$AssetDst = "$StagingDir\assets\something_like_balloon"
if (-not (Test-Path $AssetSrc)) { throw "素材フォルダが見つかりません: $AssetSrc" }
New-Item -ItemType Directory -Path "$StagingDir\assets" | Out-Null
Copy-Item $AssetSrc -Destination "$StagingDir\assets" -Recurse
Write-Host "    コピー: assets\something_like_balloon\"

# ---------------------------------------------------------------------------
# 4. zip 圧縮
# ---------------------------------------------------------------------------
Write-Host ">>> zip 圧縮 ..." -ForegroundColor Cyan
if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }
Compress-Archive -Path "$StagingDir\*" -DestinationPath $ZipPath
Write-Host "    作成: $ZipPath" -ForegroundColor Green

# ---------------------------------------------------------------------------
# 5. ステージング削除
# ---------------------------------------------------------------------------
Remove-Item $StagingDir -Recurse -Force

Write-Host ""
Write-Host "完了: $ZipName" -ForegroundColor Green
Write-Host ""
Read-Host "Enterキーで閉じる"
