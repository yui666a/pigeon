# セキュリティ残課題4件 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** セキュリティ監査後に残った将来課題4件（C10 iframe隔離 / C9 外部画像オプトイン / Riskゲート配線 / Linux secret-service）を、それぞれ独立した PR として順番に実装する。

**Architecture:** 各項目は独立サブシステム。C10/C9 はフロント（mail-view + CSP + Rust fetch コマンド）、Riskゲートは ADR 0004 の Phase 4-3〜4-5 前倒し（usecase バス）、Linux 鍵保管は secure_store.rs のバックエンド差し替え。

**Tech Stack:** React 19 + TypeScript / Vitest / Rust + Tauri 2 / rusqlite / keyring 3 (sync-secret-service) / reqwest

## Global Constraints

- GitHub Flow: main 直 push 不可。各項目 = 1 ブランチ = 1 PR（Single Concern）
- TDD: Red → Green → Refactor。フロントは Vitest、Rust は `#[cfg(test)]`
- `unwrap()`/`expect()` はテスト以外で禁止。エラーは `AppError`
- CSP の `img-src` は緩めない（C9 でも維持）
- DB マイグレーションは現在 v14 まで使用済み。新規は v15 から昇順
- Dependabot PR には触らない
- 検証コマンド: `cargo test --lib -- --test-threads=4`（src-tauri 内）/ `pnpm test`

---

## Part 1: C10 — メール本文の sandbox iframe 隔離（PR 1本）

**目的:** DOMPurify バイパス時に本文スクリプトが Tauri IPC へ到達できないようにする第3層防御。

**ブランチ:** `feat/mail-body-iframe-isolation`

### Task 1-1: 実機検証（WKWebView × CSP × srcdoc iframe）

コミット前の使い捨て検証。結果が Part 1 全体の設計を決める。

**Files（一時変更、検証後 revert）:**
- Modify: `src/App.tsx`（プローブ component を一時マウント）
- Modify: `src-tauri/src/lib.rs`（結果を stdout に出す一時 command `debug_csp_probe`）
- Modify: `src-tauri/tauri.conf.json`（CSP バリアント試行時のみ）

- [ ] **Step 1: プローブを作る** — `<iframe sandbox="allow-same-origin" srcdoc>` を生成し、(a) onload 到達 (b) contentDocument が読めるか (c) `<script>` が実行されないか、を `invoke("debug_csp_probe", { result })` で Rust 側 `println!` に送る
- [ ] **Step 2: `pnpm tauri dev` をバックグラウンド起動し stdout を確認**
- [ ] **Step 3: 判定**
  - srcdoc 表示 OK → CSP `frame-src 'none'` のまま Task 1-2 へ
  - srcdoc ブロック → CSP を `frame-src about:`（about:srcdoc のみ許可、http(s) フレームは引き続き遮断）に変えて再試行。それも不可なら `frame-src 'self' about:` 等を順に試し、採用値を記録
  - `allow-scripts` なしで script 不実行、かつ親から contentDocument が読めることを確認
- [x] **Step 4: 一時変更を revert し、検証結果をこの計画書に追記**

**検証結果（2026-07-15 実機 WKWebView / macOS）:**
`frame-src 'none'` の CSP 下でも `<iframe sandbox="allow-same-origin" srcdoc>` は正常にロードされる（プローブ出力: `onload: text=LOADED bodyLen=105`）。
- onload 発火・contentDocument 読み取り可（`allow-same-origin` が機能）
- srcdoc 内の `<script>` は実行されない（`allow-scripts` なしのため。text が SCRIPT_RAN に書き換わっていない）
- **結論: CSP は現行の `frame-src 'none'` のまま変更不要。** WKWebView は frame-src を srcdoc iframe に適用しない

### Task 1-2: srcdoc 構築ユーティリティ（TDD）

**Files:**
- Create: `src/utils/buildMailFrameSrcdoc.ts`
- Test: `src/__tests__/buildMailFrameSrcdoc.test.ts`

**Interfaces:**
- Produces: `buildMailFrameSrcdoc(sanitizedHtml: string): string` — sanitize 済み HTML を、基本スタイル（font/word-break/img max-width）付きの完全な HTML 文書文字列に包む

- [ ] sanitize 済み HTML が body に埋まる / 基本スタイルを含む / `<meta charset>` を含む、のテストを先に書く → 実装 → pass → commit

### Task 1-3: MailBody の iframe 化

**Files:**
- Modify: `src/components/mail-view/MailBody.tsx`
- Test: `src/__tests__/MailBody.test.tsx`（既存があれば拡張、なければ新設）

**実装方針:**
- `<iframe sandbox="allow-same-origin" srcdoc={buildMailFrameSrcdoc(sanitizeMailHtml(resolvedHtml))} />`。`allow-scripts` は付けない
- リンク捕捉: iframe onLoad で `contentDocument` に click リスナーを付け、既存 `handleBodyLinkClick` 相当（http/https/mailto のみ `openUrl`、それ以外は無視）を移植
- 高さ: onLoad 時に `contentDocument.body.scrollHeight` を読んで iframe height に反映。`ResizeObserver` で追従
- cid 画像解決（既存 useEffect）は iframe 注入前の HTML 文字列に対して行う（変更不要）
- text/plain フォールバック（`<pre>`）は iframe 外のまま維持

- [ ] テスト: sandbox 属性値 / srcdoc に sanitize 済み本文が入る / `allow-scripts` が含まれない、を先に書く → 実装 → `pnpm test` pass
- [ ] `pnpm tauri dev` で実メール表示・リンククリック・cid 画像・高さを目視確認
- [ ] commit → PR 作成（CSP 変更があれば同 PR に含め、理由を PR 説明に書く）

---

## Part 2: C9 — 外部画像の表示オプトイン（設計書 + PR 1本)

**目的:** 外部画像は既定で遮断のまま、ユーザーの明示操作で Rust 経由フェッチ→data URI 化して表示する。CSP `img-src` は緩めない。

**ブランチ:** `feat/external-image-optin`

### Task 2-1: 設計書を書く（設計書ファースト）

**Files:**
- Create: `docs/design/2026-07-15-external-image-optin-design.md`

決定事項（設計書に明文化）:
- 発火: メールごとの「画像を表示」ボタン（外部画像が除去された場合のみ表示）
- 永続化: v1 はしない（表示は開いている間のみ）。送信者単位の常時許可は将来課題として記載
- 取得: Rust `fetch_external_images(urls: Vec<String>) -> Vec<FetchedImage>`。reqwest / rustls / タイムアウト 10s / 1枚 5MB 上限 / 最大 20 枚 / Content-Type が `image/*` のみ / http(s) 以外拒否 / private・loopback IP へのアクセス拒否（SSRF/内部網プローブ対策）
- 置換: 取得結果を data URI として元 URL と置換（cid 置換と同型）→ 再 sanitize → 表示
- プライバシー注記: オプトインしても取得時点で開封が送信者に通知される。それを「ユーザーの明示判断」に変えるのが本機能

### Task 2-2: 外部画像 URL の抽出と置換（フロント、TDD）

**Files:**
- Create: `src/utils/externalImages.ts`
- Test: `src/__tests__/externalImages.test.ts`

**Interfaces:**
- Produces: `extractExternalImageUrls(html: string): string[]`（http/https の img src を重複除去で列挙）/ `replaceExternalImageUrls(html: string, images: { url: string; dataUri: string }[]): string`

### Task 2-3: Rust fetch コマンド（TDD)

**Files:**
- Create: `src-tauri/src/commands/remote_image_commands.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`（invoke_handler 登録）

検証ロジック（URL スキーム・IP 判定・サイズ/型検査）は純関数に切り出して単体テスト。実フェッチ部は薄く保つ。

### Task 2-4: MailBody に「画像を表示」ボタン統合

**Files:**
- Modify: `src/components/mail-view/MailBody.tsx`
- Create: `src/api/remoteImageApi.ts`
- Modify: `src/utils/sanitizeMailHtml.ts`（除去発生を検知できるようにする: 除去件数を返す関数追加 or 抽出ユーティリティで判定）

- [ ] テスト → 実装 → 目視確認 → commit → PR

---

## Part 3: Riskゲート配線（ADR 0004 Phase 4-3/4-4/4-5 前倒し、PR 2〜3本）

**目的:** MCP/Agent driver 公開前の必須基盤。Sensitive/Reversible 操作を dispatch → gate → audit に載せる。

**前提発見:** 対象操作（send/delete/archive/flag）は async（IMAP）だが UseCase::run は同期。ADR D6 の「async は実物が来た 4-5 で」の"実物"が今来たので、async バス化を先行タスクとして入れる（ADR 0004 の記述と整合、ADR に追記）。

**ブランチ構成（Stacked PR）:**
1. `feat/usecase-async-bus` — UseCase::run の async 化（async_trait）+ 既存 search/dispatch の追従
2. `feat/usecase-sensitive-extraction` — flag/unread/delete/archive/send を UseCase 化して commands から dispatch 経由に載せ替え（Phase 4-3）
3. `feat/risk-gate-audit` — ゲート本体（driver×Risk マトリクス）+ SQLite 監査シンク（migration v15）+ 承認キュー backend（migration v16）（Phase 4-4）

**ゲートマトリクス（4-4、ADR 0004 D3/D4 に従う）:**

| Risk \ Driver | Ui | Mcp | Agent |
|---|---|---|---|
| Read | 許可 | 許可 | 許可 |
| Reversible | 許可+監査 | 許可+監査 | 許可+監査 |
| Sensitive | 許可+監査（人間クリック=承認済み） | 承認キュー投入して保留 | 承認キュー投入して保留 |

**監査テーブル（v15）:** `audit_log(id, ts, use_case, risk, driver, input_summary)`
**承認キュー（v16）:** `approval_queue(id, ts, use_case, input_json, driver, status[pending/approved/rejected])`

- [ ] 各 PR とも TDD（gate のマトリクス全組合せ、SqliteAuditSink の記録、UseCase 化した操作の dispatch 経由等価性）
- [ ] ADR 0004 のステータス・フェーズ記述を実装に合わせて更新
- [ ] 実装完了後、`2026-07-14-phase4-2-usecase-bus-design.md` を archive へ移す（ADR 0004 ドキュメント運用方針）

---

## Part 4: Linux secret-service 連携（PR 1本）

**目的:** Linux でマスター鍵を FileKeyBackend（0600 ファイル）から OS の secret-service（GNOME Keyring 等）へ。

**ブランチ:** `feat/linux-secret-service`

**鍵判断:** keyring 3 の `sync-secret-service` feature は zbus ベース＝pure Rust で libdbus 不要（Cargo.toml コメントの懸念は keyring 2 時代の話）。CI（ubuntu-latest, ヘッドレス）では secret-service デーモンが居ないため、**実行時フォールバック**が必須。

**Files:**
- Modify: `src-tauri/Cargo.toml`（`[target.'cfg(target_os = "linux")'.dependencies] keyring = { version = "3", features = ["sync-secret-service"] }`、既存コメント更新）
- Modify: `src-tauri/src/secure_store.rs`
  - `KeychainBackend` の cfg を linux にも拡張
  - Linux 用 `default_master_key_backend`: secret-service を試し、利用不可なら FileKeyBackend へフォールバックする合成バックエンド
  - 既存 `master.key` ファイルからの移行: keychain が空でファイルに鍵がある場合、keychain へ書き込み成功後にファイルを削除
- Modify: `docs/adr/0003-secret-storage-boundary.md`（Linux 節の更新）

- [ ] TDD: フォールバック/移行ロジックは `MasterKeyBackend` trait のモック2つ（primary/fallback）で単体テスト
- [ ] macOS では `cargo test` + `cargo check --target x86_64-unknown-linux-gnu` は不可（リンク不要の `cargo check` も cc 依存で不安定）→ CI（ubuntu）でビルド・テストが通ることを PR で確認
- [ ] commit → PR

---

## 実行順序と完了条件

1. Part 1 (C10) → PR → CI green
2. Part 2 (C9) → PR → CI green
3. Part 3 (Riskゲート) → Stacked PR ×3 → CI green
4. Part 4 (Linux) → PR → CI green

各 PR はマージせず作成まで（マージ判断はユーザー）。
