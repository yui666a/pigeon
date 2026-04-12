# Pigeon

AIによってメールを案件ごとに自動グルーピングするデスクトップメールクライアント。ローカルLLM（Ollama）を使用してメールを案件単位で自動分類し、「案件 > スレッド > メール」の階層で管理する。

## 必要要件

- [Rust](https://www.rust-lang.org/tools/install) (1.75+)
- [Node.js](https://nodejs.org/) (20+)
- [Ollama](https://ollama.ai/) (ローカルLLM)
- Tauri 2 の[システム依存関係](https://v2.tauri.app/start/prerequisites/)

## セットアップ

```bash
# リポジトリをクローン
git clone https://github.com/yui666a/pigeon.git
cd pigeon

# フロントエンドの依存関係をインストール
npm install

# 開発サーバーを起動
npm run tauri dev
```

## ビルド

```bash
npm run tauri build
```
