# CLAUDE.md

このリポジトリで作業する際の開発ガイドです。

## プロジェクト概要

「伺か」のバルーン（吹き出し）素材を編集・プレビュー・出力する GUI エディタ。

- 言語: Rust (edition 2024, stable)
- GUI: egui / eframe 0.29
- **Windows 専用**: GDI テキスト描画・Windows API（`windows` crate）に依存
- コード内のコメント・UI 文言は日本語で統一している。

## ビルド・確認

```sh
cargo check          # コンパイル確認（通常はこれで十分）
cargo build          # デバッグビルド
cargo build --release  # リリースビルド
cargo run            # 実行
```

- コンパイルエラーの確認は `cargo check` を使う（`cargo build` まで通す必要はない）。
- ビルド時に `build.rs`（winres）でアイコンを埋め込むため Windows 前提。

## ディレクトリ構成

```
src/
  main.rs            エントリポイント・ウィンドウ初期化
  core/              ロジック層（GUI非依存）
    color.rs         配色
    composer.rs      バルーン画像合成
    descript.rs      descript.txt のパース／書き換え
    gdi_text.rs      GDI によるテキスト描画
    layout.rs        files.txt（レイアウト定義）の解釈
    preview.rs       プレビュー画像生成
    profile.rs       slbe_profile.json（色設定等）の保存・復元
    project.rs       プロジェクトの作成・コピー・装飾補完
  gui/               UI 層
    app.rs           BalloonApp 本体・各種ハンドラ
    state.rs         AppState（アプリ全状態）
    loader.rs        素材フォルダ読み込み・キャッシュ構築
    field_def.rs     装飾フィールド定義
    panels/          各 UI パネル（menubar / toolbar / preview / settings / balloon_list / files_editor）
resource/            フォント・アイコン・サンプル素材
```

## プロジェクトのデータ構成（重要）

- **txt 正本方式**: descript.txt / install.txt 等のテキストファイルを「正本」とし、アプリはそれを直接読み書きする。プロジェクトと出力に差が出ないようにする設計。
- **`profile/slbe/` 配下**: アプリ固有の補助ファイルを集約。
  - `profile/slbe/files.txt`: レイアウト定義（旧 `files.txt` から移行）。
  - `slbe_profile.json`: 色設定など、txt に表現できない情報の保存先。
- 文字コードは **UTF-8 のみ対応**。読み込み時に Shift_JIS 等は UTF-8 へ変換する。

## ブランチ運用

- **dev**: 開発用ブランチ。日々のコミットはここに行う（細かい粒度で可）。
- **main**: 公開用（GitHub default branch）。dev とは独立した系統で、各リリースで dev のツリー内容を 1 コミットとして載せる運用。

### リリース手順（read-tree 方式）

dev で版上げコミット（`Cargo.toml` の `version` 更新）を済ませた後：

```sh
git checkout main
git read-tree dev^{tree}      # インデックスを dev のツリーに一致させる
git checkout-index -a -f      # 作業ツリーへ展開
git clean -fd                 # dev に無いファイルを物理削除
git write-tree                # → dev^{tree} と同じハッシュになることを確認
git diff --cached dev         # → 空（差分なし）であることを確認
git commit -m "release: vX.Y.Z"
git tag vX.Y.Z                # lightweight タグ（半角 v）
git checkout dev
```

- **`git merge --squash dev` は使えない**（main と dev は共通祖先を持たないため "unrelated histories" で失敗する）。
- `--allow-unrelated-histories` でマージしない。
- `git checkout dev -- .` も不可（dev で削除されたファイルが main に残るため）。必ず read-tree 方式で。

### バージョン番号

- `Cargo.toml` の `version` を更新するだけでよい（表示名は `env!("CARGO_PKG_VERSION")` で自動取得）。
- 0.x 系のうちは、機能追加・修正とも基本は第3桁を回す（0.10.1, 0.10.2 …）。
  互換が壊れる変更や大きな区切りでのみ第2桁を上げる（0.11.0）。
  （semver では 0.x の MINOR/PATCH 区別は保証対象外のため、この運用で問題ない。）
