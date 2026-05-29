# Something Like Balloon Editor

伺かバルーン素材の配色・テキスト設定を編集して出力するGUIツールです。
バルーン画像の配色変更、descript.txt の各種設定編集、個別バルーン設定ファイルの生成をプレビューを確認しながら行えます。

---

## 動作環境

- Windows 10 / 11

---

## 起動方法

`balloon_editor.exe` をダブルクリックして起動します。

### フォルダ構成

```
（任意のフォルダ）/
├── balloon_editor.exe        ← 実行ファイル
├── projects/                 ← プロジェクトフォルダ（自動生成）
│   └── （プロジェクト名）/
├── sample_balloon/            ← サンプル素材フォルダ
├── output/                   ← 出力先（自動生成）
│   └── （出力物フォルダ名）/
├── state.json                ← 作業状態の自動保存（自動生成）
├── ThirdPartyNotices.txt     ← 使用ライブラリのライセンス文
├── LICENSE.txt               ← このアプリのライセンスファイル
├── README.md                 ← このファイル
└── MANUAL.md                 ← 操作マニュアル
```

プロジェクトはメニューバーの「ファイル > 新規プロジェクト...」または「ファイル > フォルダからプロジェクトを作成...」で作成できます。
`sample_balloon/` をもとにプロジェクトを作成するには「ファイル > フォルダからプロジェクトを作成...」で `sample_balloon/` を選択してください。

### アンインストール

レジストリや AppData への書き込みは行いません。アプリのフォルダをまるごと削除するだけでアンインストールできます。

詳しい使い方は [MANUAL.md](MANUAL.md) を参照してください。

---

## 作者

作者：lost_nd_xxx（ろすえん）

何か問題がございましたら、以下のどちらかからご連絡をお願いします。

- [GitHubのIssues（要アカウント）](https://github.com/lost-nd-xxx/something-like-balloon-editor/issues)
- [その他連絡先](https://lnx.flop.jp/form/postmail.html)

---

## 同梱素材について

`sample_balloon/` は、作者が別途配布しているバルーン素材を改変して同梱したものです。

- 原素材: https://github.com/lost-nd-xxx/something_like_balloon/tree/main/something_like_template
- ライセンス: CC0 1.0 Universal

この素材を使用して制作した作品の公開・配布・改変に制限はありません。

---

## ライセンス

MIT License — 詳細は [LICENSE](LICENSE) ファイルを参照してください。

### 使用ライブラリ

使用しているサードパーティクレートとそのライセンスは [ThirdPartyNotices.txt](ThirdPartyNotices.txt) を参照してください。

| クレート | 用途 | ライセンス |
|----------|------|------------|
| egui / eframe / egui_extras | GUIフレームワーク | MIT OR Apache-2.0 |
| image | 画像の合成・描画処理 | MIT OR Apache-2.0 |
| png | PNG メタ情報（tRNS チャンク等）の読み取り | MIT OR Apache-2.0 |
| serde / serde_json | JSON シリアライズ | MIT OR Apache-2.0 |
| regex | 正規表現 | MIT OR Apache-2.0 |
| anyhow | エラーハンドリング | MIT OR Apache-2.0 |
| fontdue | フォントラスタライズ | MIT OR Apache-2.0 OR Zlib |
| rfd | ファイルダイアログ | MIT |
| open | ファイル・URLを既定アプリで開く | MIT |
| trash | ファイルのゴミ箱移動 | MIT |
| encoding_rs | 文字コード変換（Shift_JIS等→UTF-8） | MIT OR Apache-2.0 |
| windows | Windows API バインディング | MIT OR Apache-2.0 |

### 使用フォント

| フォント | 用途 | ライセンス |
|----------|------|------------|
| BIZ UDPGothic | UIフォント（実行ファイルに埋め込み済み） | SIL Open Font License 1.1 |
