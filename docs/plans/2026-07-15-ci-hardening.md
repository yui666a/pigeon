# GitHub Actions サプライチェーン・ハードニング Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 公開リポジトリで Apple 署名 secrets を安全に扱えるよう、Action SHA 固定・secrets の environment 隔離・job 権限分離・npm 時限防御・リポジトリ保護設定を導入する。

**Architecture:** ワークフロー変更は `feat/ci-hardening`(base: `feat/release-cicd`、Stacked PR)。release.yml は「依存コードを実行する build(read権限)」と「書き込みだけ行う publish(write権限)」に分離し、APPLE_* は `release` environment(タグポリシー v*)の secrets に登録する。リポジトリ設定は gh api で適用し、`sha_pinning_required` のみ**マージ後**に有効化する(main 上の旧ワークフローがタグ参照のうちに有効化すると CI が全停止するため)。

**Tech Stack:** GitHub Actions, gh api (rulesets / environments / actions permissions), Dependabot, pnpm 10.16+ (`minimumReleaseAge`), actionlint, zizmor

**Spec:** `docs/design/2026-07-15-ci-hardening-design.md`

> **実行時メモ（2026-07-15）**: 本プラン作成と並行して PR #139（repo-ci-hygiene）が main にマージされ、Task 1 のうち test.yml の SHA 固定と dependabot.yml 追加は実施済みとなった。そのため Task 1 は release.yml の SHA 固定のみ実施し、SHA は test.yml と同一値に揃えた。また zizmor の指摘を受け、Task 2 に「リリースビルドでのキャッシュ不使用」「checkout の persist-credentials: false」を追加した。Task 4 の Dependabot alerts は dependabot.yml とは別の設定のため予定どおり適用した。

## Global Constraints

- ブランチ: `feat/ci-hardening`(base: `feat/release-cicd`)。PR は #140 の子として Stacked PR
- 全 `uses:` はフルコミット SHA + `# vX.Y.Z` コメント。使用 SHA は本プランに記載の値を使う(2026-07-15 時点の最新)
- `dtolnay/rust-toolchain` は SHA 固定すると ref 名によるツールチェーン選択が失われるため、`with: toolchain: stable` を必ず追加する
- workflow レベル `permissions: contents: read`。write は publish / unsigned-notice の job レベルのみ
- APPLE_* secrets の登録先は `release` environment(リポジトリ secrets には登録しない)
- 各タスクの検証: `actionlint .github/workflows/*.yml` が exit 0、`zizmor .github/workflows/` で High 以上なし(未インストールなら `brew install zizmor`)
- `sha_pinning_required: true` の適用は本 PR が main にマージされた後(Task 6)。それ以前に有効化しない

### 使用する SHA(2026-07-15 解決値)

| Action | SHA | バージョン |
|---|---|---|
| actions/checkout | `34e114876b0b11c390a56381ad16ebd13914f8d5` | v4.3.1 |
| pnpm/action-setup | `fc06bc1257f339d1d5d8b3a19a8cae5388b55320` | v4.4.0 |
| actions/setup-node | `49933ea5288caeca8642d1e84afbd3f7d6820020` | v4.4.0 |
| actions/cache | `0057852bfaa89a56745cba8c7296529d2fc39830` | v4.3.0 |
| dtolnay/rust-toolchain | `fa04a1451ff1842e2626ccb99004d0195b455a88` | master(stable) |
| Swatinem/rust-cache | `c19371144df3bb44fab255c43d04cbc2ab54d1c4` | v2.9.1 |
| actions/upload-artifact | `ea165f8d65b6e75b540449e92b4886f43607fa02` | v4.6.2 |
| actions/download-artifact | `d3f86a106a0bac45b974a628896c90dbdf5c8093` | v4.3.0 |

---

### Task 1: Action の SHA 固定と Dependabot 追従

**Files:**
- Modify: `.github/workflows/test.yml`
- Modify: `.github/workflows/release.yml`
- Create: `.github/dependabot.yml`

**Interfaces:**
- Produces: SHA 固定済みの `uses:` 行。Task 2 はこの状態の release.yml を前提に構造変更する

- [ ] **Step 1: test.yml の uses: を SHA 固定する**

対象は5箇所。以下の通り置換する(`actions/checkout@v4` は2箇所):

```yaml
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4.3.1
```

```yaml
      - uses: dtolnay/rust-toolchain@fa04a1451ff1842e2626ccb99004d0195b455a88 # stable
        with:
          toolchain: stable
```

```yaml
      - uses: Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4 # v2.9.1
        with:
          workspaces: src-tauri
```

```yaml
      - uses: pnpm/action-setup@fc06bc1257f339d1d5d8b3a19a8cae5388b55320 # v4.4.0
        with:
          version: 10
```

```yaml
      - uses: actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 # v4.4.0
        with:
          node-version: 22
```

```yaml
      - name: Cache node_modules
        uses: actions/cache@0057852bfaa89a56745cba8c7296529d2fc39830 # v4.3.0
```

- [ ] **Step 2: release.yml の uses: を同様に SHA 固定する**

対象6箇所(checkout / pnpm/action-setup / setup-node / cache / rust-toolchain / rust-cache / upload-artifact)。SHA は上表の値。`dtolnay/rust-toolchain` は `toolchain: stable` を追加して次の形にする:

```yaml
      - uses: dtolnay/rust-toolchain@fa04a1451ff1842e2626ccb99004d0195b455a88 # stable
        with:
          toolchain: stable
          targets: ${{ matrix.target }}
```

```yaml
      - name: Upload build artifact
        if: github.event_name == 'push'
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4.6.2
```

- [ ] **Step 3: .github/dependabot.yml を作成する**

```yaml
version: 2
updates:
  # SHA固定したActionをDependabotが追従更新する(SHAとバージョンコメントの両方を更新する)
  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
```

- [ ] **Step 4: 検証**

Run: `actionlint .github/workflows/test.yml .github/workflows/release.yml`
Expected: 出力なし・exit 0

Run: `zizmor .github/workflows/`(未インストールなら `brew install zizmor`)
Expected: unpinned-uses の指摘が消えている。High 以上なし

- [ ] **Step 5: コミット**

```bash
git add .github/workflows/test.yml .github/workflows/release.yml .github/dependabot.yml
git commit -m "ci(security): 全ActionをコミットSHAに固定しDependabot追従を追加"
```

---

### Task 2: release.yml の job 分離・environment 隔離・権限最小化

**Files:**
- Modify: `.github/workflows/release.yml`(全面改稿)

**Interfaces:**
- Consumes: Task 1 で SHA 固定済みの release.yml
- Produces: job `build`(contents: read, output `signed`)/ `publish`(contents: write)/ `unsigned-notice`。environment `ci` / `release` を参照(Task 4 で作成)

- [ ] **Step 1: release.yml を以下の内容に全面更新する**

```yaml
name: Release

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

# 依存コードを実行する build に書き込み権限を渡さない。write は publish 側の job レベルでのみ付与
permissions:
  contents: read

jobs:
  build:
    name: Build DMG (${{ matrix.target }})
    runs-on: macos-14
    # APPLE_* secrets は release environment(デプロイタグポリシー v*)にのみ登録してある。
    # push:main の検証ビルドは secrets を持たない ci environment で実行し、
    # 悪性依存が main に混入しても署名 secrets に触れられないようにする
    environment: ${{ github.event_name == 'release' && 'release' || 'ci' }}
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
    outputs:
      # unsigned-notice job が「未署名ビルドか」を判定するための出力
      signed: ${{ steps.signing.outputs.signed }}
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4.3.1

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

      # latest だと pnpm のメジャー更新で挙動が変わり CI が壊れるため固定(test.yml と同じ)
      - uses: pnpm/action-setup@fc06bc1257f339d1d5d8b3a19a8cae5388b55320 # v4.4.0
        with:
          version: 10

      - uses: actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 # v4.4.0
        with:
          node-version: 22

      - name: Cache node_modules
        uses: actions/cache@0057852bfaa89a56745cba8c7296529d2fc39830 # v4.3.0
        id: node-modules-cache
        with:
          path: node_modules
          key: node-modules-${{ runner.os }}-${{ hashFiles('pnpm-lock.yaml') }}

      - name: Install dependencies
        if: steps.node-modules-cache.outputs.cache-hit != 'true'
        run: pnpm install --frozen-lockfile

      - uses: dtolnay/rust-toolchain@fa04a1451ff1842e2626ccb99004d0195b455a88 # stable
        with:
          toolchain: stable
          targets: ${{ matrix.target }}

      - uses: Swatinem/rust-cache@c19371144df3bb44fab255c43d04cbc2ab54d1c4 # v2.9.1
        with:
          workspaces: src-tauri
          key: ${{ matrix.target }}

      - name: Expose signing status
        id: signing
        run: echo "signed=$HAS_APPLE_CERT" >> "$GITHUB_OUTPUT"

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

      # push(検証)では7日で消える。release では publish job がここから取得する
      - name: Upload build artifact
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4.6.2
        with:
          name: ${{ matrix.asset }}
          path: src-tauri/target/${{ matrix.target }}/release/bundle/dmg/${{ matrix.asset }}
          retention-days: 7

  # 依存コードを一切実行しない job だけに contents: write を渡す
  publish:
    name: Publish DMGs to release
    runs-on: ubuntu-latest
    needs: build
    if: github.event_name == 'release'
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@d3f86a106a0bac45b974a628896c90dbdf5c8093 # v4.3.0
        with:
          pattern: "Pigeon_*.dmg"
          merge-multiple: true
          path: dist

      - name: Upload DMGs to release
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          gh release upload "$GITHUB_REF_NAME" dist/*.dmg --clobber --repo "$GITHUB_REPOSITORY"

  # Secrets 未登録のままリリースした場合、ダウンロードした人が Gatekeeper 警告で
  # 混乱しないよう Release 本文に注意書きを自動追記する
  unsigned-notice:
    name: Add unsigned notice to release notes
    runs-on: ubuntu-latest
    needs: [build, publish]
    if: github.event_name == 'release' && needs.build.outputs.signed != 'true'
    permissions:
      contents: write
    steps:
      - name: Append unsigned notice to release body
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

変更点の要約(レビュー用): workflow 権限を read に格下げ / build job に environment 切替を追加 / `Expose signing status` ステップと `signed` output を新設(environment 移行後は unsigned-notice から `secrets.APPLE_CERTIFICATE` が見えなくなるため、build 経由で伝搬する)/ Artifacts アップロードを無条件化 / Release への添付を publish job に分離 / unsigned-notice は `needs.build.outputs.signed` で判定。

- [ ] **Step 2: 検証**

Run: `actionlint .github/workflows/release.yml && zizmor .github/workflows/`
Expected: actionlint exit 0。zizmor で High 以上なし

- [ ] **Step 3: コミット**

```bash
git add .github/workflows/release.yml
git commit -m "ci(security): 署名secretsをenvironment隔離しbuild/publishのjob権限を分離"
```

---

### Task 3: pnpm minimumReleaseAge

**Files:**
- Create: `pnpm-workspace.yaml`

**Interfaces:**
- Produces: リポジトリルートの pnpm 設定。ローカル・CI 両方の `pnpm install` に効く

- [ ] **Step 1: pnpm-workspace.yaml を作成する**

```yaml
# pnpm 10 の設定ファイル(単一パッケージだが設定置き場として使う)
# 公開後3日未満の npm パッケージをインストールしない(Shai-Hulud型の
# 公開直後マルウェアバージョン取り込みへの時限防御)。
# 緊急で新しいバージョンが必要な場合は minimumReleaseAgeExclude に追加する
minimumReleaseAge: 4320
```

- [ ] **Step 2: ローカルで install が通ることを確認する**

Run: `pnpm install --frozen-lockfile`
Expected: 成功(既存 lockfile のバージョンはすべて3日以上前のため影響なし)。`git status` で `pnpm-lock.yaml` に差分が出ないこと。差分が出た場合は内容を確認し、設定由来の軽微な変更のみなら lockfile も一緒にコミットする

- [ ] **Step 3: コミット**

```bash
git add pnpm-workspace.yaml
git commit -m "chore(security): pnpm minimumReleaseAgeで公開直後パッケージの取り込みを遅延"
```

---

### Task 4: リポジトリ設定の適用(gh api)

`sha_pinning_required` は**ここでは適用しない**(main 上の test.yml がまだタグ参照のため。Task 6 で適用)。

**Files:**
- なし(gh api によるリポジトリ設定)

**Interfaces:**
- Produces: environments `ci` / `release`(Task 2 の release.yml が参照)、main / v* タグのルールセット、フォークPR承認強化、Dependabot 有効化

- [ ] **Step 1: environments を作成する**

```bash
gh api -X PUT repos/yui666a/pigeon/environments/ci
gh api -X PUT repos/yui666a/pigeon/environments/release \
  --input - <<'EOF'
{"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}
EOF
gh api -X POST repos/yui666a/pigeon/environments/release/deployment-branch-policies \
  -f name='v*' -f type=tag
```

Expected: それぞれ 200/201 の JSON が返る

- [ ] **Step 2: フォーク PR の承認ポリシーを全外部コントリビューターに変更する**

```bash
gh api -X PUT repos/yui666a/pigeon/actions/permissions/fork-pr-contributor-approval \
  -f approval_policy=all_external_contributors
```

- [ ] **Step 3: main ブランチのルールセットを作成する**

```bash
gh api -X POST repos/yui666a/pigeon/rulesets --input - <<'EOF'
{
  "name": "protect-main",
  "target": "branch",
  "enforcement": "active",
  "conditions": { "ref_name": { "include": ["~DEFAULT_BRANCH"], "exclude": [] } },
  "rules": [
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    {
      "type": "pull_request",
      "parameters": {
        "required_approving_review_count": 0,
        "dismiss_stale_reviews_on_push": false,
        "require_code_owner_review": false,
        "require_last_push_approval": false,
        "required_review_thread_resolution": false,
        "allowed_merge_methods": ["merge", "squash", "rebase"]
      }
    },
    {
      "type": "required_status_checks",
      "parameters": {
        "strict_required_status_checks_policy": false,
        "required_status_checks": [
          { "context": "Rust Tests" },
          { "context": "Frontend Tests" }
        ]
      }
    }
  ]
}
EOF
```

注: status check の context は test.yml の job `name`(`Rust Tests` / `Frontend Tests`)と一致させること。

- [ ] **Step 4: v* タグのルールセットを作成する**

```bash
gh api -X POST repos/yui666a/pigeon/rulesets --input - <<'EOF'
{
  "name": "protect-release-tags",
  "target": "tag",
  "enforcement": "active",
  "bypass_actors": [
    { "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" }
  ],
  "conditions": { "ref_name": { "include": ["refs/tags/v*"], "exclude": [] } },
  "rules": [
    { "type": "creation" },
    { "type": "update" },
    { "type": "deletion" }
  ]
}
EOF
```

注: `actor_id: 5` は RepositoryRole の admin。オーナー自身は bypass できるため Release publish によるタグ作成は通る。

- [ ] **Step 5: Dependabot alerts / security updates を有効化する**

```bash
gh api -X PUT repos/yui666a/pigeon/vulnerability-alerts
gh api -X PUT repos/yui666a/pigeon/automated-security-fixes
```

Expected: どちらも 204 No Content

- [ ] **Step 6: 適用結果を GET で確認する**

```bash
gh api repos/yui666a/pigeon/environments -q '.environments[].name'
gh api repos/yui666a/pigeon/environments/release/deployment-branch-policies -q '.branch_policies[] | "\(.name) (\(.type))"'
gh api repos/yui666a/pigeon/actions/permissions/fork-pr-contributor-approval
gh api repos/yui666a/pigeon/rulesets -q '.[] | "\(.name): \(.enforcement)"'
gh api repos/yui666a/pigeon/vulnerability-alerts >/dev/null && echo "dependabot alerts: enabled"
```

Expected: `ci` と `release` / `v* (tag)` / `all_external_contributors` / `protect-main: active` と `protect-release-tags: active` / `dependabot alerts: enabled`

---

### Task 5: ドキュメント整合と Stacked PR 作成

**Files:**
- Modify: `docs/design/2026-07-15-release-cicd-design.md`(secrets 登録先の記述)

**Interfaces:**
- Consumes: Task 1〜4 の完成物

- [ ] **Step 1: リリースCI/CD設計書の secrets 登録先を environment に更新する**

`docs/design/2026-07-15-release-cicd-design.md` の「署名・公証（Secrets 有無で自動切替）」セクションの表の直後に以下を追記する:

```markdown
> **2026-07-15 更新**: 上記6項目の登録先はリポジトリ secrets ではなく **`release` environment の secrets**（Settings > Environments > release）。詳細は `docs/design/2026-07-15-ci-hardening-design.md` を参照。
```

- [ ] **Step 2: コミット**

```bash
git add docs/design/2026-07-15-release-cicd-design.md
git commit -m "docs(ci): secrets登録先をrelease environmentに変更（ハードニング設計に追従）"
```

- [ ] **Step 3: Stacked PR を作成する**

```bash
git push -u origin feat/ci-hardening
gh pr create \
  --base feat/release-cicd \
  --title "ci: GitHub Actionsサプライチェーン・ハードニング" \
  --body "$(cat <<'EOF'
## 概要

公開リポジトリで Apple 署名 secrets を扱う前提の CI ハードニング。

- 全 Action をコミット SHA に固定（Dependabot で追従）
- APPLE_* secrets を `release` environment（タグポリシー `v*`）に隔離。push:main ビルドからは不可視
- 依存コードを実行する build job は `contents: read`。Release への書き込みは依存コードを実行しない publish job に分離
- pnpm `minimumReleaseAge: 4320`（3日）で公開直後パッケージの取り込みを遅延
- リポジトリ設定（別途 gh api で適用済み）: フォークPR全件承認制 / main・v*タグのルールセット / Dependabot alerts

**Stacked PR**: base は #140 (`feat/release-cicd`)。#140 のマージ後に base を main に変更してからマージすること。
マージ後に `sha_pinning_required` を有効化する（設計書の Task 6 参照）。

設計書: `docs/design/2026-07-15-ci-hardening-design.md`

🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01ALC4nXGHhHjtsrpcrX4C4X
EOF
)"
```

---

### Task 6: マージ後の仕上げ（sha_pinning_required と実地検証）

両 PR（#140 → 本 PR）が main にマージされた後に実施する。

**Files:**
- なし(gh api とCI確認)

- [ ] **Step 1: sha_pinning_required を有効化する**

main 上の全ワークフローが SHA 固定済みになったことを確認してから:

```bash
grep -rEn 'uses:.*@(v[0-9]|main|master|stable)' .github/workflows/ && echo "NG: タグ参照が残っている" || echo "OK: 全てSHA固定"
gh api -X PUT repos/yui666a/pigeon/actions/permissions \
  -F enabled=true -f allowed_actions=all -F sha_pinning_required=true
gh api repos/yui666a/pigeon/actions/permissions
```

Expected: 最後の GET で `"sha_pinning_required":true`

- [ ] **Step 2: push:main の検証ビルドが ci environment で通ることを確認する**

Run: `gh run watch $(gh run list --workflow=release.yml --limit 1 --json databaseId -q '.[0].databaseId')`
Expected: build 2件が success。ジョブログの environment が `ci`、`Build DMG (unsigned)` 側が実行されている

- [ ] **Step 3: prerelease で release 経路を確認する**

リリースCI/CDプラン（`docs/plans/2026-07-15-release-cicd.md`）の Task 4 Step 3〜5 と同一手順。追加確認: release イベントの build job が environment `release` で実行され、publish job が Artifacts 経由で DMG を添付すること。

- [ ] **Step 4: (証明書取得後) APPLE_* を release environment に登録する**

Settings > Environments > release > Environment secrets に6項目を登録する（リポジトリ secrets ではない点に注意）:
`APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`

CLI の場合:

```bash
gh secret set APPLE_TEAM_ID --env release --body "<TEAM_ID>"
# 他5項目も同様。証明書は: gh secret set APPLE_CERTIFICATE --env release --body "$(base64 -i cert.p12)"
```
