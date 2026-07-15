# リリース CI/CD 設計書

- 日付: 2026-07-15
- ステータス: 承認済み
- 関連: `docs/adr/`（該当ADRなし・新規領域）、`.github/workflows/test.yml`（既存のPRテストCI）

## 目的

Pigeon を macOS 向けデスクトップアプリとして自サイト（ポートフォリオ）から配信できるようにする。具体的には:

1. main へのマージごとに macOS ビルドが通ることを CI で検証する
2. GitHub Release を publish すると、署名・公証済み（証明書登録後）の DMG が自動で Release に添付される
3. ポートフォリオサイトから固定 URL で常に最新の DMG をダウンロードできる

## 背景と制約

- リポジトリは public。`releases/latest/download/<asset>` の固定 URL が認証なしで利用できる
- Apple Developer Program は加入申請中（2026-07-15 時点で有効化待ち）。署名用証明書はまだ Secrets に登録できないため、**Secrets 未登録でもワークフローは動作し、未署名 DMG を生成する**必要がある
- 配布アーキテクチャは aarch64（Apple Silicon）と x86_64（Intel）の **2つの DMG を別々に配布**する。将来 Apple Silicon のみに絞る場合は matrix から1エントリ削除するだけで済む構成にする

## 全体構成

新規ワークフロー `.github/workflows/release.yml` を1本追加する。既存の `test.yml`（PR時の cargo test + vitest）は変更しない。

### トリガーと役割

| トリガー | 役割 | 成果物の行き先 |
|---|---|---|
| `push: main` | ビルド検証（リリース経路が壊れていないことの確認） | Actions Artifacts（保持7日） |
| `release: published` | リリース用 DMG のビルドと配布 | GitHub Release のアセット |

下書き（draft）Release では起動しない。publish されたタイミングのみをトリガーとする。

`push: main` 側は `docs/**` と `*.md` を `paths-ignore` に指定し、ドキュメントのみの変更では macOS ビルドを走らせない。

### ジョブ構成

```
release.yml
├── build (matrix: aarch64-apple-darwin, x86_64-apple-darwin)
│     runs-on: macos-14
│     1. checkout / pnpm / Rust toolchain（target 追加）/ キャッシュ
│     2. （release 時のみ）タグとバージョンの整合チェック
│     3. （Secrets 登録時のみ）署名・公証の環境変数を設定
│     4. pnpm tauri build --target <matrix.target>
│     5. DMG を固定名にリネーム
│     6. push:main → Artifacts へアップロード
│        release   → Release アセットへアップロード
└── （release 時・未署名ビルドの場合）Release ノートへ注意書きを追記
```

同一のビルド手順を両トリガーで共有することで、「main では通っていたのにリリースだけ壊れる」ズレを防ぐ。

## バージョン整合チェック

- リリースタグは `vX.Y.Z` 形式とする（例: `v0.2.0`）
- リリースジョブの先頭で、タグの `X.Y.Z` と `src-tauri/tauri.conf.json` の `version` を比較し、**不一致なら即失敗**させる
- これにより「version を上げ忘れたまま publish」した場合、DMG が添付されないまま失敗し、気づける

### リリース手順（運用フロー）

1. `tauri.conf.json`（と `package.json`）の version を上げる PR を作成しマージする
2. GitHub の Releases 画面から `vX.Y.Z` タグで Release を作成し publish する
3. CI が DMG をビルドして Release に添付する（署名・公証込みで 15〜30 分程度）

## 署名・公証（Secrets 有無で自動切替）

Tauri は以下の環境変数が設定されていると、ビルド時に codesign → notarytool 提出 → ステープルまで自動実行する。

| Secret 名 | 内容 |
|---|---|
| `APPLE_CERTIFICATE` | Developer ID Application 証明書（.p12 を base64 化） |
| `APPLE_CERTIFICATE_PASSWORD` | .p12 のパスワード |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: <名前> (<TEAM_ID>)` |
| `APPLE_ID` | Apple ID のメールアドレス |
| `APPLE_PASSWORD` | App 用パスワード |
| `APPLE_TEAM_ID` | チーム ID |

> **2026-07-15 更新**: 上記6項目の登録先はリポジトリ secrets ではなく **`release` environment の secrets**（Settings > Environments > release）。デプロイタグポリシー `v*` により、リリースタグ上の実行でしか secrets を取得できない。詳細は `docs/design/2026-07-15-ci-hardening-design.md` を参照。

- ワークフロー内で `APPLE_CERTIFICATE` Secret の有無を判定し、**登録済みの場合のみ**これらの環境変数をビルドステップに渡す。空文字の環境変数を渡すと Tauri が署名を試みて失敗する可能性があるため、未登録時は環境変数自体を設定しない
- Secrets 未登録の間は未署名 DMG が生成される（Gatekeeper 警告あり。動作確認用）
- 未署名ビルドの場合、Release 本文に「このビルドは未署名です」という注意書きを自動追記する
- Developer Program 有効化後は **Secrets を登録するだけ**で署名・公証済みリリースに切り替わる。ワークフローの変更は不要

## アセット命名とポートフォリオ連携

DMG はバージョンを含まない固定名で Release にアップロードする:

- `Pigeon_aarch64.dmg`（Apple Silicon / M1 以降）
- `Pigeon_x86_64.dmg`（Intel）

バージョンはリリースタグで判別できるため、ファイル名に含めない。これにより以下の URL が**常に最新リリースを指す**:

```
https://github.com/yui666a/pigeon/releases/latest/download/Pigeon_aarch64.dmg
https://github.com/yui666a/pigeon/releases/latest/download/Pigeon_x86_64.dmg
```

ポートフォリオサイト側はこの2つの URL を「Apple Silicon (M1以降)」「Intel」のラベル付きで並べる。ブラウザからは訪問者の Mac のアーキテクチャを確実に判定できない（Safari は Apple Silicon 上でも Intel と名乗る）ため、自動振り分けは行わずユーザーに選ばせる。

## テスト・検証方針

ワークフロー YAML はユニットテストできないため、以下で検証する:

1. `actionlint` によるローカル静的チェック
2. PR マージ後、`push: main` トリガーで検証ビルドが通ることを確認
3. テスト用 prerelease（例: `v0.1.1`）を publish し、DMG が Release に添付されること・固定 URL でダウンロードできることを確認
4. ダウンロードした未署名 DMG がローカルで起動することを確認（Gatekeeper 回避は開発者自身の操作で行う）
5. 証明書登録後、再度リリースし `spctl -a -vv` で署名・公証を確認

## スコープ外

- アプリ内自動アップデート（`tauri-plugin-updater`）— 配信開始後に別件で設計する
- Windows / Linux ビルド
- main マージごとの自動リリース（タグ自動生成）
- ユニバーサルバイナリ化（必要になれば matrix を `universal-apple-darwin` に置き換える）
