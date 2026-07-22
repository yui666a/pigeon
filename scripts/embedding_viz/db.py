"""Pigeon の SQLite DB から埋め込みベクトルとラベルを読み出す。

DB は必ず読み取り専用で開く。実 DB は本番データであり、書き込み事故は許容できない。
"""

from __future__ import annotations

import os
import sqlite3
import sys
from pathlib import Path
from typing import NamedTuple

import numpy as np
import sqlite_vec

# bge-m3 の次元数。vec_chunks の float[1024] に対応する（src-tauri/src/db/migrations.rs の v18）。
EMBEDDING_DIM = 1024


class ChunkRow(NamedTuple):
    chunk_id: int
    mail_id: str
    subject: str
    project_id: str | None
    project_name: str | None
    vector: np.ndarray


def default_db_path() -> Path:
    """src-tauri/src/lib.rs の dirs::data_dir().join("Pigeon") に対応する既定パス。"""
    if sys.platform == "darwin":
        base = Path.home() / "Library" / "Application Support"
    elif sys.platform == "win32":
        base = Path(os.environ.get("APPDATA", Path.home()))
    else:
        base = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
    return base / "Pigeon" / "pigeon.db"


def decode_vector(blob: bytes) -> np.ndarray:
    """vec_chunks.embedding の生バイト列を float32 配列へ復元する。

    Rust 側は zerocopy::IntoBytes で &[f32] をそのまま BLOB 化している
    （src-tauri/src/db/chunks.rs:82）ので、frombuffer で読み直せる。
    """
    if len(blob) % 4 != 0:
        raise ValueError(f"float32 の倍数長ではありません: {len(blob)} バイト")
    return np.frombuffer(blob, dtype=np.float32)


def connect(db_path: Path) -> sqlite3.Connection:
    """読み取り専用で接続し、sqlite-vec 拡張をロードする。

    vec0 仮想テーブル（vec_chunks）を読むには拡張のロードが必須。
    """
    if not db_path.exists():
        raise FileNotFoundError(f"DB が見つかりません: {db_path}")
    conn = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    try:
        conn.enable_load_extension(True)
        sqlite_vec.load(conn)
        conn.enable_load_extension(False)
    except AttributeError as exc:
        raise RuntimeError(
            "この Python は拡張ロードに対応していません。"
            "macOS 標準の Python ではなく mise / pyenv 等でビルドされた Python を使ってください。"
        ) from exc
    return conn


def build_query(limit: int | None, assigned_only: bool) -> tuple[str, list]:
    """チャンク取得クエリを組み立てる。

    JOIN 構造は src-tauri/src/db/vec_search.rs の search_mails_semantic を踏襲する。
    limit は必ずパラメータとして渡す（SQL 文字列に埋め込まない）。
    """
    where = "WHERE mpa.project_id IS NOT NULL" if assigned_only else ""
    limit_clause = "LIMIT ?" if limit is not None else ""
    params: list = [limit] if limit is not None else []
    sql = f"""
        SELECT v.chunk_id, c.mail_id, m.subject, mpa.project_id, p.name, v.embedding
        FROM vec_chunks v
        JOIN mail_chunks c ON c.id = v.chunk_id
        JOIN mails m ON m.id = c.mail_id
        LEFT JOIN mail_project_assignments mpa ON mpa.mail_id = m.id
        LEFT JOIN projects p ON p.id = mpa.project_id
        {where}
        ORDER BY c.mail_id, c.chunk_index
        {limit_clause}
    """
    return sql, params


def load_chunks(
    conn: sqlite3.Connection, limit: int | None, assigned_only: bool
) -> list[ChunkRow]:
    """チャンク単位でベクトルとラベルを読み出す。

    次元が想定と違う行はスキップする（モデル変更後の再埋め込み途中など）。
    """
    sql, params = build_query(limit, assigned_only)
    rows: list[ChunkRow] = []
    skipped = 0
    for chunk_id, mail_id, subject, project_id, project_name, blob in conn.execute(
        sql, params
    ):
        vector = decode_vector(blob)
        if vector.shape[0] != EMBEDDING_DIM:
            skipped += 1
            continue
        rows.append(
            ChunkRow(
                chunk_id=chunk_id,
                mail_id=mail_id,
                subject=subject or "(件名なし)",
                project_id=project_id,
                project_name=project_name,
                vector=vector,
            )
        )
    if skipped:
        print(f"警告: 次元が {EMBEDDING_DIM} でない行を {skipped} 件スキップしました")
    return rows
