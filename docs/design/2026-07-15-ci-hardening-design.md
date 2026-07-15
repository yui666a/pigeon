# GitHub Actions サプライチェーン・ハードニング設計書

- 日付: 2026-07-15
- ステータス: 承認済み
- 関連: `docs/design/2026-07-15-release-cicd-design.md`（リリースCI/CD。本書はその secrets 登録方法とジョブ構成を一部変更する）

## 目的

公開リポジトリで Apple 署名用 secrets を扱う前提のもと、GitHub Actions を経路とするシークレット窃取・サプライチェーン攻撃（tj-actions/changed-files 改ざん、Shai-Hulud 型 npm ワーム等で実際に使われた手口）への耐性を上げる。

## 脅威モデル（現状評価）

### すでに塞がっている経路

- フォークからの `pull_request` トリガーには secrets が渡らず、トークンも read-only（GitHub の仕様）。危険な `pull_request_target` / self-hosted runner は不使用
- `release.yml` のトリガー（push:main / release:published）は書き込み権限者しか起こせない
- デフォルトワークフロー権限は `read` 設定済み。pnpm 10 はデフォルトで依存の lifecycle スクリプトを実行しない

### 残存リスクと対応（重要度順）

| # | リスク | 対応 |
|---|---|---|
| 1 | サードパーティ Action の改ざん（可変タグ参照） | SHA固定 + Dependabot追従 + `sha_pinning_required` |
| 2 | 悪性依存がビルド中に APPLE_* secrets を窃取 | environment 隔離 + `minimumReleaseAge` |
| 3 | 任意コード実行 job が write トークンを保持 | build / publish の job 分離 |
| 4 | main・タグが無保護（トークン漏洩時に直接改ざん可能） | ルールセット |
| 5 | フォーク PR の Actions 実行が初回承認のみ | 全外部コントリビューター承認制 |

## 設計

### 1. Action の SHA 固定と自動追従

- `test.yml` / `release.yml` の全 `uses:` をフルコミット SHA に固定し、末尾に `# vX.Y.Z` コメントを付ける
- `.github/dependabot.yml` を追加（`package-ecosystem: github-actions`, weekly）。Dependabot は SHA ピンとバージョンコメントを両方更新する
- リポジトリ設定 `sha_pinning_required: true` を有効化し、SHA 固定なしのワークフローは実行拒否する

### 2. Apple secrets の environment 隔離

APPLE_* 6項目は**リポジトリ secrets ではなく `release` environment の secrets として登録する**（リリースCI/CD設計書の記載を本書で上書き。まだ未登録のため移行作業はない）。

- environment `release`: APPLE_* 6項目を保持。デプロイタグポリシー `v*` を設定（リリースタグ上の実行のみ secrets を取得できる）
- environment `ci`: secrets なし。push:main の検証ビルド用
- build job は `environment: ${{ github.event_name == 'release' && 'release' || 'ci' }}` で切り替える

効果: 悪性依存が main に混入しても push:main ビルドには APPLE_* が存在しない。窃取機会はオーナーが意図的に publish するリリースビルド時のみに縮小される。

### 3. トークン権限の最小化（job 分離）

「サードパーティのコードを実行する job」と「write トークンを持つ job」を重ねない。

```
release.yml
├── build (macos-14, matrix)          permissions: contents: read
│     依存コードを実行（pnpm/cargo）。署名時のみ APPLE_* を保持
│     DMG は push/release どちらでも Artifacts にアップロード
├── publish (ubuntu-latest, release時のみ, needs: build)
│     permissions: contents: write
│     Artifacts をダウンロードして gh release upload のみ。依存コードは実行しない
└── unsigned-notice (release時のみ, needs: publish)
      permissions: contents: write
```

- workflow レベルの `permissions` は `contents: read` に落とす
- タグ/バージョン整合チェックは build job に残す（publish 前に落とすため）

### 4. npm サプライチェーンの時限防御

`pnpm-workspace.yaml`（pnpm 10 の設定ファイル）を新規作成し `minimumReleaseAge: 4320`（3日 = 4320分）を設定する。公開後3日未満のバージョンをインストール対象にしないことで、公開直後に検知・削除される悪性バージョンの取り込みを防ぐ。ローカル開発と CI の両方に効く。

- 必要 pnpm: >= 10.16（ローカル 10.16.1 / CI は `version: 10` で 10.x 最新。いずれも充足）
- 緊急で新しいバージョンが必要な場合は `minimumReleaseAgeExclude` に個別パッケージを追加する

### 5. リポジトリ設定（gh api で適用し、本書に記録する）

| 設定 | 変更内容 | 適用方法 |
|---|---|---|
| SHA固定強制 | `sha_pinning_required: false → true` | `PUT /repos/{o}/{r}/actions/permissions` |
| フォークPR承認 | `first_time_contributors → all_external_contributors` | `PUT .../actions/permissions/fork-pr-contributor-approval` |
| main ルールセット | PR必須（承認数0）・force push禁止・削除禁止・status check（Rust Tests / Frontend Tests）必須 | `POST /repos/{o}/{r}/rulesets` |
| v* タグルールセット | 作成・更新・削除を制限、bypass は repo admin のみ | `POST /repos/{o}/{r}/rulesets` |
| Dependabot alerts | 有効化 | `PUT /repos/{o}/{r}/vulnerability-alerts` |
| Dependabot security updates | 有効化 | `PUT /repos/{o}/{r}/automated-security-fixes` |
| environments | `release`（タグポリシー v*）と `ci` を作成 | `PUT /repos/{o}/{r}/environments/{name}` |

補足:
- main ルールセットの status check 必須により、docs のみの PR でも test.yml が走る現状の挙動が前提になる（test.yml に paths フィルタを追加する場合はルールセットと整合を取ること）
- ルールセットはオーナー自身にも適用される（force push 不可等）。緊急時は Settings > Rules から一時的に無効化できる

## テスト・検証方針

1. `actionlint` が exit 0（SHA 固定後も含む）
2. `zizmor`（GitHub Actions 静的セキュリティ監査ツール）をローカルで実行し、High 以上の指摘がないこと
3. `pnpm install --frozen-lockfile` が `minimumReleaseAge` 設定後もローカルで成功すること
4. リポジトリ設定適用後、GET API で各設定値を確認すること
5. マージ後: push:main の検証ビルドが `ci` environment で成功すること（APPLE_* が見えない状態で unsigned ビルドになること）
6. リリース時: prerelease publish で publish job が Artifacts 経由で DMG を添付できること

## スコープ外

- StepSecurity harden-runner による egress 制御（macOS ランナー未対応）
- cargo-audit / npm audit / OpenSSF Scorecard の CI 組み込み
- CODEOWNERS・署名コミット必須化

## 実施形態

- ワークフロー・設定ファイル変更: `feat/ci-hardening` ブランチ（base: `feat/release-cicd`、Stacked PR。親 PR #140 のマージ後に base を main に付け替える）
- リポジトリ設定: PR とは独立に gh api で適用（コード化できないため本書を正とする）
