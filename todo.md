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

### 2. 署名付き初回リリース

- [ ] v0.1.2 を**正式リリース**（prerelease なし）として publish
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
