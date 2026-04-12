# 開発ガイド

## 開発フロー

1. **設計書を確認する** - `docs/superpowers/specs/` 配下の設計書を読み、変更対象の仕様を理解する
2. **テストを書く** - 実装したい機能のテストを先に書く（TDD）
3. **実装する** - テストが通るようにプロダクションコードを書く
4. **リファクタリング** - テストが通ることを確認しつつコードを整理する
5. **コミットする** - Conventional Commits 形式でコミット

## ブランチ戦略

- `main` - 安定版。直接コミットしない
- `feature/<name>` - 機能開発ブランチ
- `fix/<name>` - バグ修正ブランチ

```bash
# 機能開発の開始
git checkout -b feature/imap-connection

# 作業完了後
git push origin feature/imap-connection
# GitHub上でPRを作成
```

## コミット規約

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

`mail-sync`, `classifier`, `search`, `ui`, `db` 等、変更対象のモジュール名。

## テスト

```bash
# Rust テスト
cd src-tauri && cargo test

# フロントエンド テスト
npm test
```
