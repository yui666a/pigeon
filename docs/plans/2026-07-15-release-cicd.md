# リリース CI/CD (release.yml) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** main マージで macOS ビルドを検証し、GitHub Release publish で署名・公証(Secrets登録時)済み DMG を固定名で Release に添付するワークフローを追加する。

**Architecture:** 単一ワークフロー `.github/workflows/release.yml` に `push: main`(検証・Artifacts保存)と `release: published`(Release添付)の2トリガーを持たせ、matrix(aarch64/x86_64)で同一ビルド手順を共有する。署名は job env `HAS_APPLE_CERT` の真偽でビルドステップを切り替える(空の環境変数を Tauri に渡さないため)。

**Tech Stack:** GitHub Actions (macos-14 / ubuntu-latest), Tauri 2 CLI (`pnpm tauri build`), gh CLI (Release操作), actionlint (静的検証)

**Spec:** `docs/design/2026-07-15-release-cicd-design.md`

## Global Constraints

- リリースタグは `vX.Y.Z` 形式。タグと `src-tauri/tauri.conf.json` の `version` 不一致ならリリースジョブは即失敗
- DMG アセット名は固定: `Pigeon_aarch64.dmg` / `Pigeon_x86_64.dmg`(バージョンを含めない)
- pnpm は v10、Node は v22 に固定(既存 `test.yml` と同じ)
- macOS ランナーは `macos-14`
- `push: main` は `docs/**` と `**.md` を paths-ignore
- Secrets 未登録時は未署名ビルドとし、ワークフローは失敗させない
- 空文字の APPLE_* 環境変数をビルドステップに渡さない(Tauri が署名を試みて失敗する可能性があるため)
- ワークフロー変更は必ず `actionlint .github/workflows/release.yml` が exit 0 であることを確認してからコミットする
- job レベルの `if:` では `env` / `secrets` コンテキストが使えない。secrets 有無の分岐は「job レベル `env:` に `${{ secrets.X != '' }}` を代入 → step レベル `if: env.〜` で判定」のパターンを使う

---

### Task 1: 検証ビルドワークフロー(push: main)

`release.yml` を新規作成し、main マージ時に matrix で DMG をビルドして Artifacts に保存するところまでを作る。この時点で release トリガーは含めない(Task 2 で追加)。

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Produces: job `build`(matrix: `target`, `asset`)、step 群(checkout → pnpm/node → Rust → build → rename → upload)。Task 2/3 はこのファイルにトリガーとステップを追記する

- [ ] **Step 1: release.yml を作成する**

```yaml
name: Release

on:
  # main マージごとにリリース経路(macOSビルド)が壊れていないことを検証する
  push:
    branches: [main]
    paths-ignore:
      - "docs/**"
      - "**.md"

permissions:
  contents: write

jobs:
  build:
    name: Build DMG (${{ matrix.target }})
    runs-on: macos-14
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: aarch64-apple-darwin
            asset: Pigeon_aarch64.dmg
          - target: x86_64-apple-darwin
            asset: Pigeon_x86_64.dmg
    env:
      # job レベル if では secrets を参照できないため、step レベル if 用に真偽値へ変換する
      HAS_APPLE_CERT: ${{ secrets.APPLE_CERTIFICATE != '' }}
    steps:
      - uses: actions/checkout@v4

      # latest だと pnpm のメジャー更新で挙動が変わり CI が壊れるため固定(test.yml と同じ)
      - uses: pnpm/action-setup@v4
        with:
          version: 10

      - uses: actions/setup-node@v4
        with:
          node-version: 22

      - name: Cache node_modules
        uses: actions/cache@v4
        id: node-modules-cache
        with:
          path: node_modules
          key: node-modules-${{ runner.os }}-${{ hashFiles('pnpm-lock.yaml') }}

      - name: Install dependencies
        if: steps.node-modules-cache.outputs.cache-hit != 'true'
        run: pnpm install --frozen-lockfile

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
          key: ${{ matrix.target }}

      # 署名なしビルド。Secrets 登録後は下の signed ステップに切り替わる。
      # 空の APPLE_* 環境変数を渡すと Tauri が署名を試みて失敗しうるため、
      # env を渡さないステップとして分離している
      - name: Build DMG (unsigned)
        if: env.HAS_APPLE_CERT != 'true'
        run: pnpm tauri build --target ${{ matrix.target }}

      - name: Build DMG (signed & notarized)
        if: env.HAS_APPLE_CERT == 'true'
        run: pnpm tauri build --target ${{ matrix.target }}
        env:
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}

      # releases/latest/download/ の固定URLで参照できるよう、バージョンを含まない名前に統一する
      - name: Rename DMG to stable asset name
        run: |
          DMG_DIR="src-tauri/target/${{ matrix.target }}/release/bundle/dmg"
          DMG=$(ls "$DMG_DIR"/*.dmg)
          mv "$DMG" "$DMG_DIR/${{ matrix.asset }}"

      - name: Upload build artifact
        if: github.event_name == 'push'
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.asset }}
          path: src-tauri/target/${{ matrix.target }}/release/bundle/dmg/${{ matrix.asset }}
          retention-days: 7
```

- [ ] **Step 2: actionlint で検証する**

Run: `actionlint .github/workflows/release.yml`
Expected: 出力なし・exit 0

- [ ] **Step 3: コミット**

```bash
git add .github/workflows/release.yml
git commit -m "ci(release): mainマージ時のmacOS DMGビルド検証を追加"
```

---

### Task 2: リリーストリガーとRelease添付

`release: published` トリガー、タグ/バージョン整合チェック、Release へのアセットアップロードを追加する。

**Files:**
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: Task 1 の job `build` と matrix 変数 `target` / `asset`
- Produces: release イベントで `Pigeon_aarch64.dmg` / `Pigeon_x86_64.dmg` が Release アセットに存在する状態。Task 3 は `needs: build` でこの job に依存する

- [ ] **Step 1: on: に release トリガーを追加する**

`on:` ブロックを次のように変更(push はそのまま):

```yaml
on:
  # main マージごとにリリース経路(macOSビルド)が壊れていないことを検証する
  push:
    branches: [main]
    paths-ignore:
      - "docs/**"
      - "**.md"
  # 下書きではなく publish されたタイミングのみで起動する
  release:
    types: [published]
```

- [ ] **Step 2: タグ/バージョン整合チェックのステップを追加する**

`- uses: actions/checkout@v4` の直後に挿入:

```yaml
      # バージョンを上げ忘れたまま publish した場合にここで止める
      - name: Verify tag matches tauri.conf.json version
        if: github.event_name == 'release'
        run: |
          TAG_VERSION="${GITHUB_REF_NAME#v}"
          CONF_VERSION=$(jq -r .version src-tauri/tauri.conf.json)
          if [ "$TAG_VERSION" != "$CONF_VERSION" ]; then
            echo "::error::タグ v${TAG_VERSION} と tauri.conf.json の version ${CONF_VERSION} が一致しません。version を上げる PR をマージしてから Release を作り直してください。"
            exit 1
          fi
```

- [ ] **Step 3: Release へのアップロードステップを追加する**

`Upload build artifact` ステップの直後(job `build` の末尾)に追加:

```yaml
      - name: Upload DMG to release
        if: github.event_name == 'release'
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh release upload "$GITHUB_REF_NAME" \
            "src-tauri/target/${{ matrix.target }}/release/bundle/dmg/${{ matrix.asset }}" \
            --clobber
```

- [ ] **Step 4: actionlint で検証する**

Run: `actionlint .github/workflows/release.yml`
Expected: 出力なし・exit 0

- [ ] **Step 5: コミット**

```bash
git add .github/workflows/release.yml
git commit -m "ci(release): Release publish時のDMGビルドとアセット添付を追加"
```

---

### Task 3: 未署名ビルドの注意書きジョブ

Secrets 未登録のままリリースした場合に、Release 本文へ未署名である旨を自動追記する job を追加する。

**Files:**
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: Task 1 の job `build`(`needs: build`)、job env パターン `HAS_APPLE_CERT`
- Produces: 未署名リリースの本文末尾に警告ブロックが付く(冪等: 既に付いていれば何もしない)

- [ ] **Step 1: unsigned-notice ジョブをファイル末尾に追加する**

```yaml
  # Secrets 未登録のままリリースした場合、ダウンロードした人が Gatekeeper 警告で
  # 混乱しないよう Release 本文に注意書きを自動追記する
  unsigned-notice:
    name: Add unsigned notice to release notes
    runs-on: ubuntu-latest
    needs: build
    if: github.event_name == 'release'
    env:
      HAS_APPLE_CERT: ${{ secrets.APPLE_CERTIFICATE != '' }}
    steps:
      - name: Append unsigned notice to release body
        if: env.HAS_APPLE_CERT != 'true'
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          MARKER="このビルドは未署名です"
          BODY=$(gh release view "$GITHUB_REF_NAME" --repo "$GITHUB_REPOSITORY" --json body -q .body)
          case "$BODY" in
            *"$MARKER"*) echo "notice already present"; exit 0 ;;
          esac
          NOTICE=$'\n\n> ⚠️ **このビルドは未署名です。** 開発中の動作確認用であり、開くには Gatekeeper の警告を手動で回避する必要があります。'
          gh release edit "$GITHUB_REF_NAME" --repo "$GITHUB_REPOSITORY" --notes "${BODY}${NOTICE}"
```

- [ ] **Step 2: actionlint で検証する**

Run: `actionlint .github/workflows/release.yml`
Expected: 出力なし・exit 0

- [ ] **Step 3: コミット**

```bash
git add .github/workflows/release.yml
git commit -m "ci(release): 未署名リリースへの注意書き自動追記を追加"
```

---

### Task 4: PR 作成とマージ後の実地検証

ワークフローは PR 上では動作確認できない(push:main / release トリガーのため)。マージ後に実地検証する。

**Files:**
- なし(運用ステップ)

**Interfaces:**
- Consumes: Task 1〜3 の完成した `release.yml`

- [ ] **Step 1: PR を作成する**

```bash
git push -u origin feat/release-cicd
gh pr create \
  --title "ci: リリースCI/CD（DMG配布パイプライン）を追加" \
  --body "$(cat <<'EOF'
## 概要

GitHub Release publish で DMG を自動ビルドして Release に添付するワークフローを追加する。

- `push: main` — macOSビルド検証（Artifacts保存・7日）
- `release: published` — DMGビルド + Release添付（固定名 `Pigeon_aarch64.dmg` / `Pigeon_x86_64.dmg`）
- Apple署名・公証は Secrets（`APPLE_CERTIFICATE` 等6項目）登録時のみ有効。未登録なら未署名ビルド + Release本文に注意書き
- タグ `vX.Y.Z` と `tauri.conf.json` の version 不一致でリリースジョブは失敗

設計書: `docs/design/2026-07-15-release-cicd-design.md`

## 検証

- [x] actionlint
- [ ] マージ後: push:main の検証ビルドが通ること
- [ ] `v0.1.1` prerelease publish で DMG が添付され、固定URLでダウンロードできること

🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01ALC4nXGHhHjtsrpcrX4C4X
EOF
)"
```

- [ ] **Step 2: マージ後、push:main の検証ビルドを確認する**

Run: `gh run watch $(gh run list --workflow=release.yml --limit 1 --json databaseId -q '.[0].databaseId')`
Expected: `Build DMG (aarch64-apple-darwin)` / `Build DMG (x86_64-apple-darwin)` が success、Artifacts に DMG が2つ

- [ ] **Step 3: テスト用 prerelease で Release 経路を確認する**

GitHub UI または以下で prerelease を publish(このとき tauri.conf.json は 0.1.0 のままなので、まず version を 0.1.1 に上げる PR をマージしてから):

```bash
gh release create v0.1.1 --prerelease --title "v0.1.1 (test)" --notes "リリースパイプラインの動作確認"
```

Expected:
- release.yml が起動し、DMG 2つが Release に添付される
- `curl -fsSLI https://github.com/yui666a/pigeon/releases/latest/download/Pigeon_aarch64.dmg` が 200 を返す
- Release 本文に未署名の注意書きが追記されている(Secrets 未登録の場合)

- [ ] **Step 4: ダウンロードした DMG の起動確認**

固定URLからダウンロードし、マウント → Pigeon.app 起動を確認(未署名の間は 右クリック不可のため System Settings > Privacy & Security から許可)。

- [ ] **Step 5: (証明書取得後) Secrets 登録と署名リリース確認**

リポジトリの Settings > Secrets and variables > Actions に6項目を登録:
`APPLE_CERTIFICATE`(.p12 の base64)/ `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` / `APPLE_PASSWORD`(App用パスワード)/ `APPLE_TEAM_ID`

次のリリース後に確認:

```bash
spctl -a -vv /Applications/Pigeon.app
```

Expected: `accepted` / `source=Notarized Developer ID`
