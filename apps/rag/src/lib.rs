// qdrant_client::QdrantError is a large enum (embeds tonic::Status, request bodies, ...)
// we can't shrink from here, and it's only ever on the unhappy path.
#![allow(clippy::result_large_err)]

use qdrant_client::Payload;
use qdrant_client::Qdrant;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter, PointStruct,
    QueryPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum RagError {
    #[error("qdrant request failed: {0}")]
    Qdrant(#[from] qdrant_client::QdrantError),
    #[error("invalid payload: {0}")]
    Payload(String),
}

/// One workspace-scoped collection holds every document's chunks; requests are always
/// filtered by `workspace_id` (and usually `document_id`), so there's no need to pay for
/// per-workspace collection management overhead.
pub const COLLECTION: &str = "documents";

#[derive(Debug, Clone)]
pub struct ChunkPoint {
    pub id: Uuid,
    pub vector: Vec<f32>,
    pub workspace_id: Uuid,
    pub document_id: Uuid,
    pub chunk_index: i64,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub text: String,
    pub document_id: Uuid,
    pub chunk_index: i64,
    pub score: f32,
}

#[derive(Clone)]
pub struct RagClient {
    client: Qdrant,
}

impl RagClient {
    pub fn connect(url: &str) -> Result<Self, RagError> {
        let client = Qdrant::from_url(url).build()?;
        Ok(Self { client })
    }

    /// Creates the shared collection if it doesn't already exist. Safe to call on every
    /// startup/ingest — a fresh deployment's first document call provisions it.
    pub async fn ensure_collection(&self, vector_size: u64) -> Result<(), RagError> {
        if self.client.collection_exists(COLLECTION).await? {
            return Ok(());
        }
        self.client
            .create_collection(
                CreateCollectionBuilder::new(COLLECTION)
                    .vectors_config(VectorParamsBuilder::new(vector_size, Distance::Cosine)),
            )
            .await?;
        Ok(())
    }

    pub async fn upsert_chunks(&self, chunks: Vec<ChunkPoint>) -> Result<(), RagError> {
        let points = chunks
            .into_iter()
            .map(|chunk| {
                let payload = serde_json::json!({
                    "workspace_id": chunk.workspace_id.to_string(),
                    "document_id": chunk.document_id.to_string(),
                    "chunk_index": chunk.chunk_index,
                    "text": chunk.text,
                });
                let payload: Payload = payload
                    .try_into()
                    .map_err(|e| RagError::Payload(format!("{e:?}")))?;
                Ok(PointStruct::new(chunk.id, chunk.vector, payload))
            })
            .collect::<Result<Vec<_>, RagError>>()?;

        self.client
            .upsert_points(UpsertPointsBuilder::new(COLLECTION, points))
            .await?;
        Ok(())
    }

    /// Deletes every chunk belonging to `document_id`, scoped to `workspace_id` so a
    /// workflow in one workspace can't delete another workspace's chunks.
    pub async fn delete_document(
        &self,
        workspace_id: Uuid,
        document_id: Uuid,
    ) -> Result<(), RagError> {
        let filter = Filter::must([
            Condition::matches("workspace_id", workspace_id.to_string()),
            Condition::matches("document_id", document_id.to_string()),
        ]);
        self.client
            .delete_points(DeletePointsBuilder::new(COLLECTION).points(filter))
            .await?;
        Ok(())
    }

    /// Finds the `limit` chunks most similar to `query_vector`, scoped to `workspace_id`.
    pub async fn search(
        &self,
        workspace_id: Uuid,
        query_vector: Vec<f32>,
        limit: u64,
    ) -> Result<Vec<SearchHit>, RagError> {
        let filter = Filter::must([Condition::matches("workspace_id", workspace_id.to_string())]);

        let response = self
            .client
            .query(
                QueryPointsBuilder::new(COLLECTION)
                    .query(query_vector)
                    .filter(filter)
                    .limit(limit)
                    .with_payload(true),
            )
            .await?;

        Ok(response
            .result
            .into_iter()
            .filter_map(|point| {
                let text = point.payload.get("text")?.as_str()?.to_string();
                let document_id = point
                    .payload
                    .get("document_id")?
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())?;
                let chunk_index = point.payload.get("chunk_index")?.as_integer()?;
                Some(SearchHit {
                    text,
                    document_id,
                    chunk_index,
                    score: point.score,
                })
            })
            .collect())
    }
}

/// Long-term agent memory lives in its own collection, keyed by `workspace_id` +
/// `agent_key` (an arbitrary caller-chosen identifier — e.g. a node id, or a name shared
/// across nodes/workflows that should draw on the same memory) rather than `document_id`.
pub const MEMORIES_COLLECTION: &str = "agent_memories";

#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub id: Uuid,
    pub vector: Vec<f32>,
    pub workspace_id: Uuid,
    pub agent_key: String,
    pub text: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub text: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub score: f32,
}

impl RagClient {
    pub async fn ensure_memories_collection(&self, vector_size: u64) -> Result<(), RagError> {
        if self.client.collection_exists(MEMORIES_COLLECTION).await? {
            return Ok(());
        }
        self.client
            .create_collection(
                CreateCollectionBuilder::new(MEMORIES_COLLECTION)
                    .vectors_config(VectorParamsBuilder::new(vector_size, Distance::Cosine)),
            )
            .await?;
        Ok(())
    }

    /// Persists one fact/result for `agent_key` in `workspace_id`.
    pub async fn remember(&self, entry: MemoryEntry) -> Result<(), RagError> {
        let payload = serde_json::json!({
            "workspace_id": entry.workspace_id.to_string(),
            "agent_key": entry.agent_key,
            "text": entry.text,
            "created_at": entry.created_at.to_rfc3339(),
        });
        let payload: Payload = payload
            .try_into()
            .map_err(|e| RagError::Payload(format!("{e:?}")))?;

        self.client
            .upsert_points(UpsertPointsBuilder::new(
                MEMORIES_COLLECTION,
                vec![PointStruct::new(entry.id, entry.vector, payload)],
            ))
            .await?;
        Ok(())
    }

    /// Finds the `limit` memories most similar to `query_vector` for this `agent_key`,
    /// scoped to `workspace_id`.
    pub async fn recall(
        &self,
        workspace_id: Uuid,
        agent_key: &str,
        query_vector: Vec<f32>,
        limit: u64,
    ) -> Result<Vec<MemoryHit>, RagError> {
        let filter = Filter::must([
            Condition::matches("workspace_id", workspace_id.to_string()),
            Condition::matches("agent_key", agent_key.to_string()),
        ]);

        let response = self
            .client
            .query(
                QueryPointsBuilder::new(MEMORIES_COLLECTION)
                    .query(query_vector)
                    .filter(filter)
                    .limit(limit)
                    .with_payload(true),
            )
            .await?;

        Ok(response
            .result
            .into_iter()
            .filter_map(|point| {
                let text = point.payload.get("text")?.as_str()?.to_string();
                let created_at = point
                    .payload
                    .get("created_at")?
                    .as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))?;
                Some(MemoryHit {
                    text,
                    created_at,
                    score: point.score,
                })
            })
            .collect())
    }
}

/// Splits `text` into overlapping chunks of at most `chunk_size` characters, breaking on a
/// whitespace boundary near the limit when possible so words aren't split mid-word.
/// Character-based rather than token-based — simple and dependency-free; a token-aware
/// chunker (matching the embedding model's tokenizer) would pack chunks more precisely.
pub fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    assert!(
        chunk_size > overlap,
        "chunk_size must be greater than overlap"
    );
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + chunk_size).min(chars.len());
        if end < chars.len()
            && let Some(boundary) = (start + chunk_size / 2..end)
                .rev()
                .find(|&i| chars[i].is_whitespace())
        {
            end = boundary;
        }

        let chunk: String = chars[start..end].iter().collect();
        let trimmed = chunk.trim();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }

        if end >= chars.len() {
            break;
        }
        start = end.saturating_sub(overlap);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_text_splits_long_text_with_overlap() {
        let text = "word ".repeat(100);
        let chunks = chunk_text(&text, 50, 10);

        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.len() <= 50);
        }
    }

    #[test]
    fn chunk_text_short_text_is_one_chunk() {
        let chunks = chunk_text("hello world", 50, 10);
        assert_eq!(chunks, vec!["hello world".to_string()]);
    }

    #[test]
    fn chunk_text_empty_input_is_no_chunks() {
        assert!(chunk_text("", 50, 10).is_empty());
    }

    #[test]
    fn chunk_text_covers_whole_input() {
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let chunks = chunk_text(&text, 80, 15);
        let rejoined: String = chunks.join(" ");
        for word in text.split_whitespace() {
            assert!(
                rejoined.contains(word),
                "missing word {word:?} from chunked output"
            );
        }
    }
}
