import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

const MAP_LABEL = "embedding-map";

/**
 * 埋め込みマップウィンドウを開く。既に開いていればフォーカスするだけ。
 * 多重起動を防ぐため必ずこの関数を通す。
 */
export async function openEmbeddingMapWindow(): Promise<void> {
  const existing = await WebviewWindow.getByLabel(MAP_LABEL);
  if (existing) {
    await existing.setFocus();
    return;
  }
  const win = new WebviewWindow(MAP_LABEL, {
    url: "visualization.html",
    title: "埋め込みマップ",
    width: 1100,
    height: 800,
  });
  void win.once("tauri://error", (e) => {
    console.error("埋め込みマップウィンドウの生成に失敗", e);
  });
}
