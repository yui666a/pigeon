# TODO

## リリース配信の残タスク（2026-07-15 時点）

### 1. Apple Developer Program 有効化後（メール待ち、あと1〜2日）

- [ ] developer.apple.com で **Developer ID Application 証明書**を作成し Keychain に取り込む
      （CSR は Keychain Access > 証明書アシスタントで作成）
- [ ] 証明書をパスワード付き .p12 でエクスポート
- [ ] appleid.apple.com で **App用パスワード**を発行
- [ ] APPLE_* 6項目を **release environment の secrets** に登録（リポジトリ secrets ではない）:

```bash
gh secret set APPLE_CERTIFICATE --env release --body "$(base64 -i 証明書.p12)"
gh secret set APPLE_CERTIFICATE_PASSWORD --env release --body "<p12のパスワード>"
gh secret set APPLE_SIGNING_IDENTITY --env release --body "Developer ID Application: <名前> (<TEAM_ID>)"
gh secret set APPLE_ID --env release --body "haiso666@gmail.com"
gh secret set APPLE_PASSWORD --env release --body "<App用パスワード>"
gh secret set APPLE_TEAM_ID --env release --body "<TEAM_ID>"
```

※ Claude に「secrets 登録して」と言えば対話的に進められる

### 1.5. OAuth クライアント定数を release environment に登録（配布アプリで Google 認証を通すため必須）

配布バイナリには `.env` が同梱されないため、OAuth クライアント ID/シークレットを
ビルド時にバイナリへ焼き込む（Rust の `option_env!` + `build.rs` の rerun-if-env-changed、
CI の build ステップで env 注入。実装済み）。値は `.env` の DESKTOP 2項目と同じもの。

- [ ] PIGEON_GOOGLE_* 2項目を **release environment の secrets** に登録:

```bash
gh secret set PIGEON_GOOGLE_CLIENT_ID_DESKTOP --env release --body "<.env の client id>"
gh secret set PIGEON_GOOGLE_CLIENT_SECRET_DESKTOP --env release --body "<.env の client secret>"
```

※ 未登録のままリリースすると、配布アプリで「client id not set」になり Google 認証ができない
※ シークレットをローテーションする場合は、GCP Console 更新 → `.env` 更新 → この secret も更新

### 2. 署名付き初回リリース

- [ ] tauri.conf.json 等の version を 0.1.2 に上げる PR をマージ
- [ ] **ドラフトリリース v0.1.2 を publish**（release-drafter がマージごとに自動更新している。
      Releases 画面にカテゴリ分類済みノートが下書きされている）
      → 署名・公証済み DMG が自動添付される
- [ ] ダウンロードした app で `spctl -a -vv /Applications/Pigeon.app` が
      `accepted / source=Notarized Developer ID` になることを確認
- [ ] `releases/latest/download/` の固定URLが 200 を返すことを確認
      （正式リリースが1つ以上できた時点で有効になる）

### 3. ポートフォリオサイト

- [ ] 以下の固定URLを「Apple Silicon (M1以降)」「Intel」のラベル付きでリンク:
  - https://github.com/yui666a/pigeon/releases/latest/download/Pigeon_aarch64.dmg
  - https://github.com/yui666a/pigeon/releases/latest/download/Pigeon_x86_64.dmg

### 4. その他

- [ ] Dependabot PR #141〜#153 の扱いを判断（ユーザー判断。特に #141〜#144 の
      Actions メジャー更新は Node 20 非推奨警告の解消になる）
- [ ] （将来）tauri-plugin-updater によるアプリ内自動アップデート
- [ ] （将来）Windows / Linux ビルド

## 参照

- 設計書: `docs/design/2026-07-15-release-cicd-design.md` / `docs/design/2026-07-15-ci-hardening-design.md`
- プラン: `docs/plans/2026-07-15-release-cicd.md` / `docs/plans/2026-07-15-ci-hardening.md`
- 注意: main への直接 push は不可（ruleset protect-main）。変更はすべて PR 経由
