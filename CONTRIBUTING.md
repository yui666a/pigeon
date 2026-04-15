# 開発ガイド

## 前提条件

```bash
# mise でツールチェーンをインストール
mise install

# 依存関係をインストール
pnpm install
```

## 開発フロー

1. **設計書を確認する** - `docs/superpowers/specs/` 配下の設計書を読み、変更対象の仕様を理解する
2. **テストを書く** - 実装したい機能のテストを先に書く（TDD）
3. **実装する** - テストが通るようにプロダクションコードを書く
4. **リファクタリング** - テストが通ることを確認しつつコードを整理する
5. **コミットする** - Conventional Commits 形式でコミット

## Git 戦略

本プロジェクトは GitHub Flow を採用する。

- `main` は安定版ブランチとして扱い、直接コミットしない
- `main` への取り込みは必ず Pull Request（PR）経由で行う
- PR は 1 つの変更目的（Single Concern）に限定する
- 変更目的と無関係な「ついでのリファクタリング」は別 PR に分離する
- 大きな変更は Stacked PR（依存 PR を連結した構成）で分割してよい

### PR 作成ルール

- 1 PR = 1 目的を守る
- 同じ目的の達成に必要なテスト追加・ドキュメント更新は同一 PR に含めてよい
- Stacked PR の場合は、PR 説明に依存関係（親 PR / 子 PR）を明記する
- 各 PR は単体でレビュー可能なサイズと説明を保つ

### ブランチ命名

- `feature/<name>` - 機能開発ブランチ
- `fix/<name>` - バグ修正ブランチ
- `docs/<name>` - ドキュメント更新ブランチ
- `chore/<name>` - ツール・設定変更ブランチ

```bash
# 機能開発の開始
git checkout -b feature/imap-connection

# 作業完了後
git push origin feature/imap-connection
# GitHub上でPRを作成
```

## コミット規約

- 作業完了後に 1 コミットへまとめるのではなく、意味のある変更単位でコミットする
- 目安は「1コミット = 1意図（理由を説明できる最小単位）」
- レビュー時に履歴を追えることを優先し、途中でも区切ってコミットする

Conventional Commits 形式:

```
<type>(<scope>): <description>
```

### type

| type | 用途 |
|------|------|
| feat | 新機能 |
| fix | バグ修正 |
| docs | ドキュメント変更 |
| style | コードスタイル変更（動作に影響しない） |
| refactor | リファクタリング |
| test | テストの追加・修正 |
| chore | ビルド・ツール設定等 |

### scope

`mail-sync`, `classifier`, `search`, `ui`, `db`, `commands` 等、変更対象のモジュール名。

## テスト

```bash
# Rust テスト
cd src-tauri && cargo test

# フロントエンド テスト
pnpm test

# Rust リント
cd src-tauri && cargo clippy -- -D warnings
cd src-tauri && cargo fmt -- --check
```

## 開発サーバー

```bash
pnpm tauri dev
```
