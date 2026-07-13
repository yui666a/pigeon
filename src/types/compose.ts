/** Compose画面の起動モード（新規・返信・全員に返信・転送） */
export type ComposeMode = "new" | "reply" | "replyAll" | "forward";

/** 作成中に添付したファイル（パスは Rust が送信時に読み込む） */
export interface ComposeAttachment {
  path: string;
  name: string;
  size: number;
}
