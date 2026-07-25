# 埋め込みマップ Phase B（片付ける）実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 埋め込みマップウィンドウ内で未分類の点を案件パネルへドラッグ＆ドロップして割り当て、メインウィンドウの表示へイベントで同期する。

**Architecture:** Phase A（PR #238）の `embedding-map` ウィンドウ・`MapPoint`・Canvas 散布図を土台に、(1) 案件パネル用の軽量 command `embedding_map_projects`、(2) ウィンドウ専用のマウスイベント式 D&D（メインの `useMailDrag` 方式を移植。zustand `dragStore` は別ウィンドウと共有できないためローカル state）、(3) 既存 `bulk_move_mails` の直 invoke + `emit("mail-assigned")` → メイン側 `listen` で表示反映、を載せる。設計書 `docs/design/2026-07-22-embedding-map-window-design.md` §4.4・§4.5 に対応する。

**Tech Stack:** Tauri 2（イベント: `@tauri-apps/api/event` の `emit`/`listen`）、Rust + rusqlite、React 19 + TypeScript、Vitest + React Testing Library、cargo test

## Global Constraints

- TDD 必須（Red → Green → Refactor）。テストを先に書き、失敗を確認してから実装する
- Rust: `unwrap()`/`expect()` はテストコード以外で使用しない。エラーは `AppError`
- TypeScript: `any` 禁止。Tauri invoke のレスポンスには必ず型を付ける。1ファイル1コンポーネント
- コミットは Conventional Commits 形式（`feat(ui): ...` / `test(db): ...` 等）、1コミット = 1意図
- 楽観的更新はしない: 割り当ては command 成功時のみ点の見た目を更新する（設計書 §7）
- ズーム / パン / ホバーは本計画のスコープ外（Phase A 計画の「見送り」を維持。後続タスク）
- フロントのテスト収集範囲は `src/**/*.{test,spec}.{ts,tsx}`（`vitest.config.ts`）。この範囲外にテストを置かない
- 実行コマンドはリポジトリルートで `pnpm test`（フロント）、`src-tauri/` で `cargo test`（Rust）

## 前提（Phase A で存在するもの）

- `src-tauri/src/commands/embedding_map_commands.rs`: `embedding_map_points` / `mail_preview`
- `src/visualization.tsx`: 別ウィンドウのエントリ（`VisualizationRoot` コンポーネント同居）
- `src/components/embedding-map/`: `EmbeddingMapCanvas.tsx`（クリックのみ、800×800 固定）・`PreviewPane.tsx`・`mapGeometry.ts`（`hitTest` 等）・`openMapWindow.ts`
- `src/types/embeddingMap.ts`: `MapPoint` / `MailPreview`
- `src/api/embeddingMapApi.ts`: `points()` / `preview(mailId)`
- メイン側既存: `mailApi.bulkMoveMails(mailIds, projectId)`（`bulk_move_mails` の型付きラッパ、`BulkResult { succeeded, failed }` を返す）、`useMailStore.removeUnclassifiedMail` / `fetchThreadsByProject`、`useMailDrag`（5px 閾値の自前 D&D）、`ProjectListItem` の `onMouseUp` ドロップ受け
- capability `default.json` は `"windows": ["main", "embedding-map"]` 済み。イベント emit/listen は `core:default` に含まれる

---

### Task 1: Rust — 全案件の一覧取得（db 層）

マップは全アカウント横断で点を出す（`embedding_map_points` にアカウント絞りが無い）ため、案件パネルも全アカウントの未アーカイブ案件を出す。既存 `list_projects(conn, account_id)` はアカウント絞り付きなので、絞りなし版を追加する。

**Files:**
- Modify: `src-tauri/src/db/projects.rs`（`list_projects` の直後に関数追加、`#[cfg(test)] mod tests` にテスト追加）

**Interfaces:**
- Produces: `pub fn list_all_active_projects(conn: &Connection) -> Result<Vec<Project>, AppError>`（Task 2 が使う）

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/db/projects.rs` の `mod tests` 末尾に追加:

```rust
#[test]
fn test_list_all_active_projects_excludes_archived_and_sorts_by_name() {
    let conn = setup_db();

    let b = insert_project_with_id(&conn, "pb", "acc1", "Bravo", None, None, None).unwrap();
    let a = insert_project_with_id(&conn, "pa", "acc1", "Alpha", None, None, None).unwrap();
    let archived = insert_project_with_id(&conn, "pc", "acc1", "Zulu", None, None, None).unwrap();
    archive_project(&conn, &archived.id).unwrap();

    let all = list_all_active_projects(&conn).unwrap();
    assert_eq!(
        all.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        vec![a.id.as_str(), b.id.as_str()]
    );
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test test_list_all_active_projects`
Expected: コンパイルエラー（`list_all_active_projects` 未定義）

- [ ] **Step 3: 最小実装**

`list_projects` の直後に追加（`row_to_project` マッパーを再利用）:

```rust
/// 全アカウントの未アーカイブ案件を名前順で返す。
/// 埋め込みマップの案件パネル用（マップは全アカウント横断のため絞らない）。
pub fn list_all_active_projects(conn: &Connection) -> Result<Vec<Project>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, name, description, color, is_archived, parent_id, created_at, updated_at
         FROM projects
         WHERE is_archived = FALSE
         ORDER BY name",
    )?;
    let rows = stmt.query_map([], row_to_project)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}
```

注意: `list_projects` 本体のエラー変換の書き方（`map_err` か `?` か）を見て、同じ書き方に揃えること。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test test_list_all_active_projects`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/projects.rs
git commit -m "feat(db): 全アカウント横断の未アーカイブ案件一覧を追加"
```

---

### Task 2: Rust — command `embedding_map_projects`

**Files:**
- Modify: `src-tauri/src/commands/embedding_map_commands.rs`（末尾の `mod tests` の前に追加）
- Modify: `src-tauri/src/lib.rs`（`generate_handler!` の `commands::embedding_map_commands::mail_preview,` の次行に登録）

**Interfaces:**
- Consumes: Task 1 の `list_all_active_projects`
- Produces: command `embedding_map_projects() -> Vec<MapProject>`、`MapProject { id: String, name: String, color: Option<String> }`（フロント Task 3 が型を写す）

- [ ] **Step 1: 実装**

ロジックは Task 1 の db テストで担保済みで、この command は DTO への詰め替えのみ（`embedding_map_points` と同じ薄さ）のため command 単体のテストは追加しない。

`embedding_map_commands.rs` に追加:

```rust
/// 案件パネル（D&D ドロップ先）用の軽量な案件情報。
#[derive(serde::Serialize)]
pub struct MapProject {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[tauri::command]
pub fn embedding_map_projects(db: State<DbState>) -> Result<Vec<MapProject>, AppError> {
    let projects = db.with_conn(crate::db::projects::list_all_active_projects)?;
    Ok(projects
        .into_iter()
        .map(|p| MapProject {
            id: p.id,
            name: p.name,
            color: p.color,
        })
        .collect())
}
```

`lib.rs` の `generate_handler!` リストに追加:

```rust
            commands::embedding_map_commands::embedding_map_projects,
```

- [ ] **Step 2: ビルドと全 Rust テストの確認**

Run: `cd src-tauri && cargo test`
Expected: 全 PASS（既存テスト含む）

- [ ] **Step 3: コミット**

```bash
git add src-tauri/src/commands/embedding_map_commands.rs src-tauri/src/lib.rs
git commit -m "feat(search): 埋め込みマップの案件パネル用commandを追加"
```

---

### Task 3: フロント — 型と API ラッパ

**Files:**
- Modify: `src/types/embeddingMap.ts`（`MapProject` 追加）
- Modify: `src/types/events.ts`（`MailAssignedEvent` 追加）
- Modify: `src/api/embeddingMapApi.ts`（`projects()` 追加）

**Interfaces:**
- Produces: `MapProject`（Task 4〜7 が使う）、`MailAssignedEvent`（Task 5・8 が使う）、`embeddingMapApi.projects(): Promise<MapProject[]>`（Task 7 が使う）

型のみ・薄いラッパのみのためテストは書かない（既存 api ラッパと同じ扱い）。

- [ ] **Step 1: 型を追加**

`src/types/embeddingMap.ts` 末尾に追加:

```ts
/** 案件パネル（ドロップ先）用の軽量な案件情報。Rust 側 MapProject と対 */
export interface MapProject {
  id: string;
  name: string;
  color: string | null;
}
```

`src/types/events.ts` 末尾に追加:

```ts
/** "mail-assigned" イベント（埋め込みマップでの手動割り当て）のペイロード。
 *  別ウィンドウが emit し、メインウィンドウが listen して表示へ反映する */
export interface MailAssignedEvent {
  mail_id: string;
  project_id: string;
}
```

- [ ] **Step 2: API ラッパを追加**

`src/api/embeddingMapApi.ts` を修正:

```ts
import { invokeCommand } from "./client";
import type { MapPoint, MailPreview, MapProject } from "../types/embeddingMap";

/**
 * 埋め込みマップ系 Tauri commands の型付きラッパ。
 */
export const embeddingMapApi = {
  /** 分類済み・未分類を含む全メールの埋め込み座標を取得する */
  points: () => invokeCommand<MapPoint[]>("embedding_map_points"),

  /** 点クリック時の軽量プレビュー（件名・送信者・本文冒頭）を取得する */
  preview: (mailId: string) =>
    invokeCommand<MailPreview>("mail_preview", { mailId }),

  /** 案件パネル用の全案件一覧（全アカウント・未アーカイブ・名前順） */
  projects: () => invokeCommand<MapProject[]>("embedding_map_projects"),
};
```

- [ ] **Step 3: 型チェックとコミット**

Run: `pnpm build`（`tsc && vite build`）
Expected: エラーなし

```bash
git add src/types/embeddingMap.ts src/types/events.ts src/api/embeddingMapApi.ts
git commit -m "feat(ui): 埋め込みマップの案件一覧・割り当てイベントの型を追加"
```

---

### Task 4: フロント — 割り当て結果を点へ反映する純関数

**Files:**
- Create: `src/components/embedding-map/mapAssignment.ts`
- Test: `src/components/embedding-map/mapAssignment.test.ts`

**Interfaces:**
- Consumes: `MapPoint`（既存）、`MapProject`（Task 3）
- Produces: `applyAssignment(points: MapPoint[], mailId: string, project: MapProject): MapPoint[]`（Task 7 が使う）

- [ ] **Step 1: 失敗するテストを書く**

`src/components/embedding-map/mapAssignment.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { applyAssignment } from "./mapAssignment";
import type { MapPoint, MapProject } from "../../types/embeddingMap";

const point = (mailId: string): MapPoint => ({
  x: 0,
  y: 0,
  mail_id: mailId,
  subject: `件名${mailId}`,
  project_id: null,
  project_name: null,
  project_color: null,
});

const project: MapProject = { id: "p1", name: "案件A", color: "#ff0000" };

describe("applyAssignment", () => {
  it("該当する点に案件ラベルと色を反映する", () => {
    const next = applyAssignment([point("m1"), point("m2")], "m1", project);
    expect(next[0].project_id).toBe("p1");
    expect(next[0].project_name).toBe("案件A");
    expect(next[0].project_color).toBe("#ff0000");
  });

  it("他の点は変更しない", () => {
    const next = applyAssignment([point("m1"), point("m2")], "m1", project);
    expect(next[1].project_id).toBeNull();
  });

  it("該当が無ければ配列内容は変わらない", () => {
    const points = [point("m1")];
    const next = applyAssignment(points, "unknown", project);
    expect(next).toEqual(points);
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test -- mapAssignment`
Expected: FAIL（モジュール未作成）

- [ ] **Step 3: 最小実装**

`src/components/embedding-map/mapAssignment.ts`:

```ts
import type { MapPoint, MapProject } from "../../types/embeddingMap";

/**
 * 割り当て成功後の点の見た目更新。該当する点の案件ラベルと色を差し替えた
 * 新しい配列を返す（楽観的更新はしない — command 成功後にのみ呼ぶこと）。
 */
export function applyAssignment(
  points: MapPoint[],
  mailId: string,
  project: MapProject,
): MapPoint[] {
  return points.map((p) =>
    p.mail_id === mailId
      ? {
          ...p,
          project_id: project.id,
          project_name: project.name,
          project_color: project.color,
        }
      : p,
  );
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test -- mapAssignment`
Expected: PASS（3件）

- [ ] **Step 5: コミット**

```bash
git add src/components/embedding-map/mapAssignment.ts src/components/embedding-map/mapAssignment.test.ts
git commit -m "feat(ui): 割り当て結果をマップの点へ反映する純関数を追加"
```

---

### Task 5: フロント — 割り当て実行 + イベント emit（依存注入で単体テスト可能に）

`bulk_move_mails` invoke → 成功判定 → `emit("mail-assigned")` の一連を、Tauri API を直接 import しない関数に切り出す。ウィンドウ実体（Task 7）はこれに実物の `mailApi.bulkMoveMails` と `emit` を渡す。

**Files:**
- Create: `src/components/embedding-map/assignMail.ts`
- Test: `src/components/embedding-map/assignMail.test.ts`

**Interfaces:**
- Consumes: `BulkResult`（`src/types/mail`）、`MapProject`（Task 3）、`MailAssignedEvent`（Task 3）
- Produces: `assignAndNotify(mailId, project, deps): Promise<"assigned" | "failed">`（Task 7 が使う）

- [ ] **Step 1: 失敗するテストを書く**

`src/components/embedding-map/assignMail.test.ts`:

```ts
import { describe, it, expect, vi } from "vitest";
import { assignAndNotify } from "./assignMail";
import type { BulkResult } from "../../types/mail";
import type { MapProject } from "../../types/embeddingMap";

const project: MapProject = { id: "p1", name: "案件A", color: null };

const okResult: BulkResult = { succeeded: ["m1"], failed: [] };
const ngResult: BulkResult = { succeeded: [], failed: [["m1", "boom"]] };

describe("assignAndNotify", () => {
  it("成功したら mail-assigned を emit して assigned を返す", async () => {
    const emit = vi.fn().mockResolvedValue(undefined);
    const bulkMove = vi.fn().mockResolvedValue(okResult);
    const outcome = await assignAndNotify("m1", project, { bulkMove, emit });
    expect(outcome).toBe("assigned");
    expect(bulkMove).toHaveBeenCalledWith(["m1"], "p1");
    expect(emit).toHaveBeenCalledWith("mail-assigned", {
      mail_id: "m1",
      project_id: "p1",
    });
  });

  it("command が失敗を返したら emit せず failed を返す", async () => {
    const emit = vi.fn();
    const bulkMove = vi.fn().mockResolvedValue(ngResult);
    const outcome = await assignAndNotify("m1", project, { bulkMove, emit });
    expect(outcome).toBe("failed");
    expect(emit).not.toHaveBeenCalled();
  });

  it("invoke が例外を投げたら failed を返す", async () => {
    const emit = vi.fn();
    const bulkMove = vi.fn().mockRejectedValue(new Error("ipc error"));
    const outcome = await assignAndNotify("m1", project, { bulkMove, emit });
    expect(outcome).toBe("failed");
    expect(emit).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test -- assignMail`
Expected: FAIL（モジュール未作成）

- [ ] **Step 3: 最小実装**

`src/components/embedding-map/assignMail.ts`:

```ts
import type { BulkResult } from "../../types/mail";
import type { MapProject } from "../../types/embeddingMap";
import type { MailAssignedEvent } from "../../types/events";

export type AssignOutcome = "assigned" | "failed";

interface AssignDeps {
  /** 実体は mailApi.bulkMoveMails（bulk_move_mails の直 invoke） */
  bulkMove: (mailIds: string[], projectId: string) => Promise<BulkResult>;
  /** 実体は @tauri-apps/api/event の emit */
  emit: (event: string, payload: MailAssignedEvent) => Promise<void>;
}

/**
 * 1通をドロップ先の案件へ割り当て、成功時のみ mail-assigned を emit する。
 * 別ウィンドウは zustand を共有しないため store（useMailStore.bulkMoveMails）
 * を経由せず command を直接叩く（設計書 §4.4）。
 */
export async function assignAndNotify(
  mailId: string,
  project: MapProject,
  deps: AssignDeps,
): Promise<AssignOutcome> {
  try {
    const result = await deps.bulkMove([mailId], project.id);
    if (!result.succeeded.includes(mailId)) return "failed";
    await deps.emit("mail-assigned", { mail_id: mailId, project_id: project.id });
    return "assigned";
  } catch {
    return "failed";
  }
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test -- assignMail`
Expected: PASS（3件）

- [ ] **Step 5: コミット**

```bash
git add src/components/embedding-map/assignMail.ts src/components/embedding-map/assignMail.test.ts
git commit -m "feat(ui): マップからの割り当て実行とmail-assigned emitを追加"
```

---

### Task 6: フロント — 案件パネル（ドロップ先）コンポーネント

**Files:**
- Create: `src/components/embedding-map/ProjectPanel.tsx`
- Test: `src/components/embedding-map/ProjectPanel.test.tsx`

**Interfaces:**
- Consumes: `MapProject`（Task 3）
- Produces: `<ProjectPanel projects dropActive onDrop />`（Task 7 が使う）。`onDrop: (project: MapProject) => void` は **ドラッグ中（`dropActive`）の mouseup でのみ**発火

- [ ] **Step 1: 失敗するテストを書く**

`src/components/embedding-map/ProjectPanel.test.tsx`:

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ProjectPanel } from "./ProjectPanel";
import type { MapProject } from "../../types/embeddingMap";

const projects: MapProject[] = [
  { id: "p1", name: "案件A", color: "#ff0000" },
  { id: "p2", name: "案件B", color: null },
];

describe("ProjectPanel", () => {
  it("案件名を一覧表示する", () => {
    render(<ProjectPanel projects={projects} dropActive={false} onDrop={vi.fn()} />);
    expect(screen.getByText("案件A")).toBeInTheDocument();
    expect(screen.getByText("案件B")).toBeInTheDocument();
  });

  it("ドラッグ中に mouseup した案件を onDrop へ渡す", () => {
    const onDrop = vi.fn();
    render(<ProjectPanel projects={projects} dropActive={true} onDrop={onDrop} />);
    fireEvent.mouseUp(screen.getByText("案件A"));
    expect(onDrop).toHaveBeenCalledWith(projects[0]);
  });

  it("ドラッグ中でなければ mouseup しても onDrop を呼ばない", () => {
    const onDrop = vi.fn();
    render(<ProjectPanel projects={projects} dropActive={false} onDrop={onDrop} />);
    fireEvent.mouseUp(screen.getByText("案件A"));
    expect(onDrop).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test -- ProjectPanel`
Expected: FAIL（モジュール未作成）

- [ ] **Step 3: 最小実装**

`src/components/embedding-map/ProjectPanel.tsx`:

```tsx
import type { MapProject } from "../../types/embeddingMap";

const DEFAULT_PROJECT_COLOR = "#6b7280";

interface Props {
  projects: MapProject[];
  /** 点をドラッグ中か。true のときだけドロップを受け付け、見た目も受け入れ可能を示す */
  dropActive: boolean;
  onDrop: (project: MapProject) => void;
}

/**
 * マップウィンドウ内の案件パネル（D&D ドロップ先）。
 * メインのサイドバー（ProjectListItem）と同じ mouseup 方式でドロップを受ける。
 */
export function ProjectPanel({ projects, dropActive, onDrop }: Props) {
  return (
    <div className="border-b">
      <div className="px-3 py-2 text-xs font-semibold text-gray-500">
        {dropActive ? "ここにドロップで割り当て" : "案件"}
      </div>
      <ul className="max-h-72 overflow-y-auto">
        {projects.map((p) => (
          <li
            key={p.id}
            onMouseUp={() => {
              if (dropActive) onDrop(p);
            }}
            className={`flex items-center gap-2 px-3 py-1 text-sm select-none ${
              dropActive ? "cursor-copy hover:bg-blue-50" : ""
            }`}
          >
            <span
              className="inline-block h-2.5 w-2.5 shrink-0 rounded-full"
              style={{ backgroundColor: p.color ?? DEFAULT_PROJECT_COLOR }}
            />
            <span className="truncate">{p.name}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test -- ProjectPanel`
Expected: PASS（3件）

- [ ] **Step 5: コミット**

```bash
git add src/components/embedding-map/ProjectPanel.tsx src/components/embedding-map/ProjectPanel.test.tsx
git commit -m "feat(ui): 埋め込みマップに案件パネル（ドロップ先）を追加"
```

---

### Task 7: フロント — 点のドラッグとウィンドウ配線

Canvas の点を 5px 閾値でドラッグ開始（閾値未満はこれまで通りクリック = プレビュー）し、ゴースト表示 → 案件パネルへドロップ → `assignAndNotify` → 成功時に `applyAssignment` で再着色する。あわせて `VisualizationRoot` を専用ファイルへ切り出す（テスト・責務分離のため。`visualization.tsx` はブートストラップのみ残す）。

**Files:**
- Create: `src/components/embedding-map/usePointDrag.ts`
- Create: `src/components/embedding-map/VisualizationRoot.tsx`（`visualization.tsx` から移動 + 配線追加）
- Modify: `src/components/embedding-map/EmbeddingMapCanvas.tsx`（`onPointClick` → `onPointMouseDown` に変更）
- Modify: `src/visualization.tsx`（ブートストラップのみに縮小）

このタスクはマウスイベントの合成と Canvas（jsdom では 2D context が無い）に跨るため、単体テストは Task 4〜6 の純関数・コンポーネントで担保済みの部分に任せ、ここは結合の配線のみとする。動作は Task 9 の実機確認で検証する。

**Interfaces:**
- Consumes: `usePointDrag`（本タスク内）、`ProjectPanel`（Task 6）、`assignAndNotify`（Task 5）、`applyAssignment`（Task 4）、`embeddingMapApi.projects()`（Task 3）、`mailApi.bulkMoveMails`（既存）、`emit`（`@tauri-apps/api/event`）
- Produces: なし（最終配線）

- [ ] **Step 1: usePointDrag フックを実装**

`src/components/embedding-map/usePointDrag.ts`:

```ts
import { useState } from "react";
import type { MapPoint } from "../../types/embeddingMap";

const DRAG_THRESHOLD = 5;

export interface PointDrag {
  point: MapPoint;
  /** ゴースト表示用のマウス位置（clientX/clientY） */
  x: number;
  y: number;
}

/**
 * マップの点のドラッグ。メイン側 useMailDrag と同じ 5px 閾値方式だが、
 * 別ウィンドウでは dragStore（zustand）を共有できないためローカル state で持つ。
 * 閾値未満の mouseup はクリック（onClick）として扱う。
 */
export function usePointDrag(onClick: (point: MapPoint) => void) {
  const [drag, setDrag] = useState<PointDrag | null>(null);

  const startPress = (point: MapPoint, e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const start = { x: e.clientX, y: e.clientY };
    let started = false;

    const handleMouseMove = (me: MouseEvent) => {
      const dx = me.clientX - start.x;
      const dy = me.clientY - start.y;
      if (!started && Math.abs(dx) + Math.abs(dy) > DRAG_THRESHOLD) {
        started = true;
        window.getSelection()?.removeAllRanges();
      }
      if (started) setDrag({ point, x: me.clientX, y: me.clientY });
    };

    const handleMouseUp = () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
      if (!started) onClick(point);
      // ドロップ先の onMouseUp（React ルート内）は window リスナーより先に
      // 発火するため、ここで消しても取りこぼさない
      setDrag(null);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
  };

  return { drag, startPress };
}
```

- [ ] **Step 2: Canvas を mousedown 方式へ変更**

`src/components/embedding-map/EmbeddingMapCanvas.tsx` の Props とハンドラを差し替え:

```tsx
interface Props {
  points: MapPoint[];
  /** 点の上で mousedown したとき（クリックかドラッグかは呼び出し側が判定） */
  onPointMouseDown: (point: MapPoint, e: React.MouseEvent) => void;
}
```

`handleClick` を `handleMouseDown` にリネームし、中身の最後を `if (hit) onPointMouseDown(hit, e);` に変更。JSX は `onClick={handleClick}` → `onMouseDown={handleMouseDown}` に変更。描画ロジック（useEffect）は変更しない。

- [ ] **Step 3: VisualizationRoot を切り出して配線**

`src/components/embedding-map/VisualizationRoot.tsx`（現 `visualization.tsx` の `VisualizationRoot` を移動して拡張）:

```tsx
import { useEffect, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { embeddingMapApi } from "../../api/embeddingMapApi";
import { mailApi } from "../../api/mailApi";
import { errorMessage } from "../../api/errors";
import { EmbeddingMapCanvas } from "./EmbeddingMapCanvas";
import { PreviewPane } from "./PreviewPane";
import { ProjectPanel } from "./ProjectPanel";
import { usePointDrag } from "./usePointDrag";
import { assignAndNotify } from "./assignMail";
import { applyAssignment } from "./mapAssignment";
import type { MapPoint, MapProject, MailPreview } from "../../types/embeddingMap";

export function VisualizationRoot() {
  const [points, setPoints] = useState<MapPoint[]>([]);
  const [projects, setProjects] = useState<MapProject[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [assignError, setAssignError] = useState<string | null>(null);
  const [preview, setPreview] = useState<MailPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  useEffect(() => {
    embeddingMapApi.points().then(setPoints).catch((e) => setError(errorMessage(e)));
    embeddingMapApi.projects().then(setProjects).catch((e) => setError(errorMessage(e)));
  }, []);

  const handlePointClick = (point: MapPoint) => {
    setPreviewLoading(true);
    embeddingMapApi
      .preview(point.mail_id)
      .then(setPreview)
      .catch((e) => setError(errorMessage(e)))
      .finally(() => setPreviewLoading(false));
  };

  const { drag, startPress } = usePointDrag(handlePointClick);

  const handleDrop = async (project: MapProject) => {
    if (!drag) return;
    const mailId = drag.point.mail_id;
    setAssignError(null);
    const outcome = await assignAndNotify(mailId, project, {
      bulkMove: mailApi.bulkMoveMails,
      emit,
    });
    if (outcome === "assigned") {
      setPoints((prev) => applyAssignment(prev, mailId, project));
    } else {
      setAssignError("割り当てに失敗しました");
    }
  };

  if (error) return <div className="p-4 text-red-600">エラー: {error}</div>;

  return (
    <div className="flex h-screen">
      <div className="flex-1 flex items-center justify-center overflow-hidden">
        <EmbeddingMapCanvas points={points} onPointMouseDown={startPress} />
      </div>
      <div className="w-80 border-l overflow-y-auto">
        <ProjectPanel projects={projects} dropActive={!!drag} onDrop={handleDrop} />
        {assignError && (
          <div className="px-3 py-2 text-xs text-red-600">{assignError}</div>
        )}
        <PreviewPane preview={preview} loading={previewLoading} />
      </div>
      {drag && (
        <div
          className="pointer-events-none fixed z-50 rounded bg-gray-800 px-2 py-1 text-xs text-white opacity-80"
          style={{ left: drag.x + 12, top: drag.y + 12 }}
        >
          {drag.point.subject}
        </div>
      )}
    </div>
  );
}
```

`src/visualization.tsx` はブートストラップのみに縮小:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import "./App.css"; // main.tsx はCSSを直接importせず App.tsx が読む ./App.css がグローバルCSS（Tailwind込み）のため合わせる
import { VisualizationRoot } from "./components/embedding-map/VisualizationRoot";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <VisualizationRoot />
  </React.StrictMode>,
);
```

- [ ] **Step 4: 型チェック + 全フロントテスト**

Run: `pnpm build && pnpm test`
Expected: ビルド成功、全テスト PASS

- [ ] **Step 5: コミット**

```bash
git add src/components/embedding-map/usePointDrag.ts src/components/embedding-map/VisualizationRoot.tsx src/components/embedding-map/EmbeddingMapCanvas.tsx src/visualization.tsx
git commit -m "feat(ui): マップの点をD&Dで案件へ割り当てられるようにする"
```

---

### Task 8: フロント（メイン側）— mail-assigned の listen と表示反映

**Files:**
- Modify: `src/stores/mailStore.ts`（`handleMailAssigned` と `initMailAssignedListener` を追加）
- Modify: `src/App.tsx`（listener の配線）
- Test: `src/__tests__/MailAssignedListener.test.ts`

**Interfaces:**
- Consumes: `MailAssignedEvent`（Task 3）、既存 `removeUnclassifiedMail` / `fetchThreadsByProject` / `useProjectStore.selectedProjectId`
- Produces: `handleMailAssigned(payload: MailAssignedEvent): void`、`initMailAssignedListener(): Promise<() => void>`

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/MailAssignedListener.test.ts`:

```ts
import { describe, it, expect, vi } from "vitest";
import { useMailStore } from "../stores/mailStore";
import { useProjectStore } from "../stores/projectStore";
import type { Mail } from "../types/mail";

function makeMail(id: string): Mail {
  return {
    id, account_id: "acc1", folder: "INBOX",
    message_id: `<${id}@example.com>`, in_reply_to: null, references: null,
    from_addr: "alice@example.com", to_addr: "bob@example.com",
    cc_addr: null, subject: `件名${id}`,
    body_text: "本文", body_html: null,
    date: "2026-07-24T10:00:00+09:00", has_attachments: false,
    raw_size: null, uid: 1, flags: null, is_read: false, is_flagged: false,
    fetched_at: "2026-07-24T00:00:00",
  };
}

describe("handleMailAssigned", () => {
  it("未分類リストから該当メールを除去する", () => {
    useMailStore.setState({
      unclassifiedMails: [makeMail("m1"), makeMail("m2")],
      unclassifiedThreads: [],
    });
    useProjectStore.setState({ selectedProjectId: null });

    useMailStore.getState().handleMailAssigned({ mail_id: "m1", project_id: "p1" });

    expect(useMailStore.getState().unclassifiedMails.map((m) => m.id)).toEqual(["m2"]);
  });

  it("割り当て先の案件を表示中ならスレッド一覧を取り直す", () => {
    const fetchThreadsByProject = vi.fn().mockResolvedValue(undefined);
    useMailStore.setState({
      unclassifiedMails: [],
      unclassifiedThreads: [],
      fetchThreadsByProject,
    });
    useProjectStore.setState({ selectedProjectId: "p1" });

    useMailStore.getState().handleMailAssigned({ mail_id: "m1", project_id: "p1" });

    expect(fetchThreadsByProject).toHaveBeenCalledWith("p1");
  });

  it("別の案件を表示中ならスレッド一覧は取り直さない", () => {
    const fetchThreadsByProject = vi.fn().mockResolvedValue(undefined);
    useMailStore.setState({
      unclassifiedMails: [],
      unclassifiedThreads: [],
      fetchThreadsByProject,
    });
    useProjectStore.setState({ selectedProjectId: "other" });

    useMailStore.getState().handleMailAssigned({ mail_id: "m1", project_id: "p1" });

    expect(fetchThreadsByProject).not.toHaveBeenCalled();
  });
});
```

注意: `makeMail` の `Mail` 型フィールドは `src/types/mail.ts` の現在の定義に合わせること（フィールドが増えていたら追加する。`src/__tests__/MailHeader.test.tsx` の `makeMail` が参考）。

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test -- MailAssignedListener`
Expected: FAIL（`handleMailAssigned` 未定義）

- [ ] **Step 3: mailStore に実装**

`src/stores/mailStore.ts` の import に `MailAssignedEvent` を追加:

```ts
import type { SyncProgress, NewMailEvent, MailAssignedEvent } from "../types/events";
```

（既存の events import 行に合わせて統合すること）

`MailState` interface の `initNewMailListener` 宣言の近くに追加:

```ts
  /** 埋め込みマップでの手動割り当て（mail-assigned）を表示へ反映する */
  handleMailAssigned: (payload: MailAssignedEvent) => void;
  initMailAssignedListener: () => Promise<() => void>;
```

実装（`initNewMailListener` の実装の直後に追加）:

```ts
  handleMailAssigned: (payload) => {
    // 割り当て済みになったメールを未分類リストから除去する
    get().removeUnclassifiedMail(payload.mail_id);
    // 割り当て先の案件を表示中なら一覧を取り直して新着行を反映する
    const viewing = useProjectStore.getState().selectedProjectId;
    if (viewing === payload.project_id) {
      void get().fetchThreadsByProject(payload.project_id);
    }
  },

  initMailAssignedListener: async () => {
    // 埋め込みマップウィンドウが割り当て成功時に emit する（設計書
    // 2026-07-22-embedding-map-window-design.md §4.4）
    const unlisten = await listen<MailAssignedEvent>("mail-assigned", (event) => {
      get().handleMailAssigned(event.payload);
    });
    return unlisten;
  },
```

注意: `useProjectStore` は mailStore で import 済みか確認し、無ければ追加する（`applyAssignmentApproved` が既に使っているため通常は import 済み）。

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test -- MailAssignedListener`
Expected: PASS（3件）

- [ ] **Step 5: App.tsx に配線**

`src/App.tsx` の `initNewMailListener` の useEffect の直後に追加:

```tsx
  // 埋め込みマップウィンドウでの割り当てを未分類リスト・案件ビューへ反映する
  useEffect(() => {
    const promise = initMailAssignedListener();
    return () => {
      promise.then((unlisten) => unlisten());
    };
  }, [initMailAssignedListener]);
```

セレクタも追加:

```tsx
  const initMailAssignedListener = useMailStore((s) => s.initMailAssignedListener);
```

- [ ] **Step 6: 全テスト + コミット**

Run: `pnpm test && pnpm build`
Expected: 全 PASS・ビルド成功

```bash
git add src/stores/mailStore.ts src/App.tsx src/__tests__/MailAssignedListener.test.ts
git commit -m "feat(ui): mail-assignedイベントで未分類リストと案件ビューを同期"
```

---

### Task 9: 仕上げ — 全テスト・実機確認・PR

- [ ] **Step 1: 全テストの通し実行**

Run: `pnpm test && pnpm build && (cd src-tauri && cargo test && cargo fmt --check && cargo clippy -- -D warnings)`
Expected: 全 PASS（clippy/fmt の実行要否・オプションは CI ワークフロー `.github/workflows/` の定義に合わせる）

- [ ] **Step 2: 実機確認（設計書 §10 の確認方法から Phase B 分）**

`CI=true pnpm tauri dev` で起動し、以下を目視確認:

1. マップウィンドウ右上に案件パネルが出て、全案件が色ドット付きで並ぶ
2. 未分類の点をクリック → 従来どおりプレビューが出る（ドラッグ誤発火しない）
3. 未分類の点を案件パネルへドラッグ → ゴーストが追従 → ドロップで点がその案件の色に変わる
4. メインウィンドウの未分類リストから該当メールが消える
5. メインで割り当て先の案件を表示中なら、スレッド一覧に反映される

- [ ] **Step 3: PR 作成**

PRタイトル: `feat(search): 埋め込みマップからのD&D割り当てとメイン画面同期（Phase B）`
※ タイトルは Conventional Commits 形式必須（CI の Validate PR title が落ちる）。本文に Phase A（PR #238）の後続である旨と設計書・本計画へのリンクを書く。

```bash
git push -u origin feat/embedding-map-phase-b
gh pr create --title "..." --body "..."
```

---

## Phase B で意図的に見送ったもの（後続）

- **ズーム / パン / ホバー**: Phase A から継続して見送り。密集部の操作性が問題になったら別タスクで
- **複数点の一括ドラッグ**: 1点ずつで開始。まとめ割り当ての需要が見えたら拡張
- **未読バッジ等のカウント同期**: mail-assigned では未分類リスト除去と案件ビュー再取得のみ。サイドバーの件数バッジはアプリの既存更新経路（次回取得時）に任せる
- **案件パネルの階層表示**: フラット名前順で開始（YAGNI）

## 完了後

Phase B マージで設計書 `2026-07-22-embedding-map-window-design.md` のスコープ（見る + 片付ける）が完成する。issue #208 の第2段階をクローズできる状態になる。
