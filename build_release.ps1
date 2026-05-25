# build_release.ps1
# リリースビルドを行い、配布用 zip を作成するスクリプト
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ProjectRoot = $PSScriptRoot
$AppName     = "balloon_editor"
$ReleaseName = "something-like-balloon-editor"
$ExePath     = "$ProjectRoot\target\release\$AppName.exe"

# バージョンを Cargo.toml から取得
$Version     = (Select-String -Path "$ProjectRoot\Cargo.toml" -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value
$ZipName     = "${ReleaseName}_v${Version}.zip"
$ZipPath     = "$ProjectRoot\$ZipName"
$StagingDir  = "$ProjectRoot\target\release\_staging"
# zip 内に balloon_editor/ フォルダを1段掘る
$InnerDir    = "$StagingDir\$AppName"

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
New-Item -ItemType Directory -Path $InnerDir | Out-Null

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
    Copy-Item $f -Destination $InnerDir
    Write-Host "    コピー: $(Split-Path $f -Leaf)"
}

# resource\assets\ をフォルダごとコピー
$AssetSrc = "$ProjectRoot\resource\assets"
if (-not (Test-Path $AssetSrc)) { throw "素材フォルダが見つかりません: $AssetSrc" }
Copy-Item $AssetSrc -Destination "$InnerDir\assets" -Recurse
Write-Host "    コピー: assets\"

# ---------------------------------------------------------------------------
# 4. zip 圧縮
# ---------------------------------------------------------------------------
Write-Host ">>> zip 圧縮 ..." -ForegroundColor Cyan
if (Test-Path $ZipPath) { Remove-Item $ZipPath -Force }
# StagingDir ごと圧縮することで zip 内に balloon_editor/ フォルダが1段入る
Compress-Archive -Path "$StagingDir\*" -DestinationPath $ZipPath
Write-Host "    作成: $ZipPath" -ForegroundColor Green

# ---------------------------------------------------------------------------
# 5. ステージング削除
# ---------------------------------------------------------------------------
Remove-Item $StagingDir -Recurse -Force

Write-Host ""
Write-Host "完了: $ZipName  (展開すると $AppName\ フォルダが作成されます)" -ForegroundColor Green
Write-Host ""
Read-Host "Enterキーで閉じる"
