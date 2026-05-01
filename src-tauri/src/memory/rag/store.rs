use crate::memory::rag::RagHit;
use crate::memory::schema::{MemoryError, RAG_DIR, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct VecStore {
    conn: Connection,
    dim: usize,
}

impl VecStore {
    pub fn open(project_root: &Path, dim: usize) -> Result<Self> {
        let dir = project_root.join(".reflex").join(RAG_DIR);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("vectors.db");
        let conn = Connection::open(&path)
            .map_err(|e| MemoryError::Other(format!("sqlite open: {e}")))?;
        Self::init(&conn)?;
        Self::maybe_load_vec(&conn);
        Ok(Self { conn, dim })
    }

    #[cfg(test)]
    pub fn open_in_memory(dim: usize) -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| MemoryError::Other(format!("sqlite open: {e}")))?;
        Self::init(&conn)?;
        Ok(Self { conn, dim })
    }

    fn init(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS docs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                source TEXT,
                chunk_index INTEGER NOT NULL,
                chunk TEXT NOT NULL,
                embedding BLOB NOT NULL,
                created_at_ms INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_docs_doc_id ON docs(doc_id);",
        )
        .map_err(|e| MemoryError::Other(format!("sqlite init: {e}")))?;
        Ok(())
    }

    fn maybe_load_vec(_conn: &Connection) {
        if std::env::var("REFLEX_USE_SQLITE_VEC").ok().as_deref() != Some("1") {
            return;
        }
        // Best-effort load; the rusqlite "loadable_extension" feature is not enabled here,
        // so this is a no-op stub. Manual cosine remains the default search path.
    }

    pub fn upsert(
        &self,
        doc_id: &str,
        kind: &str,
        source: Option<&Path>,
        chunks: &[(String, Vec<f32>)],
    ) -> Result<()> {
        for (_, v) in chunks {
            if v.len() != self.dim {
                return Err(MemoryError::Rag(format!(
                    "embedding dim mismatch: expected {}, got {}",
                    self.dim,
                    v.len()
                )));
            }
        }
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let source_str: Option<String> = source.map(|p| p.to_string_lossy().to_string());

        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| MemoryError::Other(format!("sqlite tx: {e}")))?;
        tx.execute("DELETE FROM docs WHERE doc_id = ?1", params![doc_id])
            .map_err(|e| MemoryError::Other(format!("sqlite delete: {e}")))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO docs (doc_id, kind, source, chunk_index, chunk, embedding, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(|e| MemoryError::Other(format!("sqlite prepare: {e}")))?;
            for (i, (text, vec)) in chunks.iter().enumerate() {
                let blob = vec_to_blob(vec);
                stmt.execute(params![
                    doc_id,
                    kind,
                    source_str,
                    i as i64,
                    text,
                    blob,
                    now_ms
                ])
                .map_err(|e| MemoryError::Other(format!("sqlite insert: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| MemoryError::Other(format!("sqlite commit: {e}")))?;
        Ok(())
    }

    pub fn search(&self, query_vec: &[f32], limit: usize) -> Result<Vec<RagHit>> {
        if query_vec.len() != self.dim {
            return Err(MemoryError::Rag(format!(
                "query dim mismatch: expected {}, got {}",
                self.dim,
                query_vec.len()
            )));
        }
        let qn = norm(query_vec);
        if qn == 0.0 {
            return Ok(Vec::new());
        }
        let mut stmt = self
            .conn
            .prepare("SELECT doc_id, kind, source, chunk, embedding FROM docs")
            .map_err(|e| MemoryError::Other(format!("sqlite prepare: {e}")))?;
        let rows = stmt
            .query_map([], |row| {
                let doc_id: String = row.get(0)?;
                let kind: String = row.get(1)?;
                let source: Option<String> = row.get(2)?;
                let chunk: String = row.get(3)?;
                let blob: Vec<u8> = row.get(4)?;
                Ok((doc_id, kind, source, chunk, blob))
            })
            .map_err(|e| MemoryError::Other(format!("sqlite query: {e}")))?;

        let mut scored: Vec<(f32, RagHit)> = Vec::new();
        for row in rows {
            let (doc_id, kind, source, chunk, blob) =
                row.map_err(|e| MemoryError::Other(format!("sqlite row: {e}")))?;
            let v = blob_to_vec(&blob);
            if v.len() != self.dim {
                continue;
            }
            let vn = norm(&v);
            if vn == 0.0 {
                continue;
            }
            let score = dot(query_vec, &v) / (qn * vn);
            let source_path: Option<PathBuf> = source.map(PathBuf::from);
            scored.push((
                score,
                RagHit {
                    doc_id,
                    source: source_path,
                    chunk,
                    score,
                    kind,
                },
            ));
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored.into_iter().map(|(_, h)| h).collect())
    }

    pub fn forget(&self, doc_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM docs WHERE doc_id = ?1", params![doc_id])
            .map_err(|e| MemoryError::Other(format!("sqlite delete: {e}")))?;
        Ok(())
    }

    #[cfg(test)]
    pub fn count(&self) -> Result<i64> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM docs", [], |r| r.get(0))
            .optional()
            .map_err(|e| MemoryError::Other(format!("sqlite count: {e}")))?
            .unwrap_or(0);
        Ok(n)
    }
}

fn vec_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn blob_to_vec(b: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(b.len() / 4);
    let mut i = 0;
    while i + 4 <= b.len() {
        let arr = [b[i], b[i + 1], b[i + 2], b[i + 3]];
        out.push(f32::from_le_bytes(arr));
        i += 4;
    }
    out
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut s = 0.0f32;
    for i in 0..a.len().min(b.len()) {
        s += a[i] * b[i];
    }
    s
}

fn norm(a: &[f32]) -> f32 {
    let mut s = 0.0f32;
    for x in a {
        s += x * x;
    }
    s.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_orders_by_similarity() {
        let store = VecStore::open_in_memory(3).unwrap();
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![0.9_f32, 0.1, 0.0];
        let c = vec![0.0_f32, 1.0, 0.0];
        store
            .upsert(
                "doc-a",
                "reference",
                None,
                &[("alpha".to_string(), a.clone())],
            )
            .unwrap();
        store
            .upsert(
                "doc-b",
                "reference",
                None,
                &[("beta".to_string(), b.clone())],
            )
            .unwrap();
        store
            .upsert(
                "doc-c",
                "reference",
                None,
                &[("gamma".to_string(), c.clone())],
            )
            .unwrap();

        let query = vec![1.0_f32, 0.0, 0.0];
        let hits = store.search(&query, 3).unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].doc_id, "doc-a");
        assert_eq!(hits[1].doc_id, "doc-b");
        assert_eq!(hits[2].doc_id, "doc-c");
        assert!((hits[0].score - 1.0).abs() < 1e-5);
        assert!(hits[2].score.abs() < 1e-5);
    }

    #[test]
    fn upsert_replaces_existing_doc() {
        let store = VecStore::open_in_memory(2).unwrap();
        store
            .upsert(
                "doc-1",
                "reference",
                None,
                &[
                    ("a".to_string(), vec![1.0, 0.0]),
                    ("b".to_string(), vec![0.0, 1.0]),
                ],
            )
            .unwrap();
        assert_eq!(store.count().unwrap(), 2);
        store
            .upsert(
                "doc-1",
                "reference",
                None,
                &[("c".to_string(), vec![1.0, 1.0])],
            )
            .unwrap();
        assert_eq!(store.count().unwrap(), 1);
        store.forget("doc-1").unwrap();
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn dim_mismatch_errors() {
        let store = VecStore::open_in_memory(3).unwrap();
        let err = store.upsert("d", "k", None, &[("x".into(), vec![1.0, 0.0])]);
        assert!(err.is_err());
    }

    #[test]
    fn blob_roundtrip() {
        let v = vec![1.0_f32, -0.5, 0.25, 1234.5];
        let b = vec_to_blob(&v);
        let r = blob_to_vec(&b);
        assert_eq!(v, r);
    }
}
