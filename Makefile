.PHONY: help build up down shell \
       test test-rust test-frontend \
       lint lint-rust fmt \
       ollama ollama-pull \
       clean clean-volumes

# デフォルトターゲット
help: ## ヘルプを表示
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

# ==============================================
# ビルド・起動
# ==============================================

build: ## Docker イメージをビルド
	docker compose build

up: ## 開発コンテナ + Ollama を起動
	docker compose up -d

down: ## コンテナを停止
	docker compose down

shell: ## 開発コンテナのシェルに入る
	docker compose run --rm dev bash

# ==============================================
# テスト
# ==============================================

test: test-rust test-frontend ## 全テストを実行

test-rust: ## Rust テストを実行
	docker compose run --rm test-rust

test-frontend: ## フロントエンド テストを実行
	docker compose run --rm test-frontend

# ==============================================
# リント・フォーマット
# ==============================================

lint: lint-rust ## 全リントを実行

lint-rust: ## Rust リント (clippy + fmt check)
	docker compose run --rm lint-rust

fmt: ## Rust コードをフォーマット
	docker compose run --rm -w /app/src-tauri dev cargo fmt

# ==============================================
# Ollama
# ==============================================

ollama: ## Ollama を起動
	docker compose up -d ollama

ollama-pull: ## 分類用モデルをダウンロード (デフォルト: gemma2)
	docker compose exec ollama ollama pull $(or $(MODEL),gemma2)

# ==============================================
# クリーンアップ
# ==============================================

clean: ## コンテナとネットワークを削除
	docker compose down --remove-orphans

clean-volumes: ## コンテナ・ボリュームを全て削除
	docker compose down --remove-orphans --volumes
