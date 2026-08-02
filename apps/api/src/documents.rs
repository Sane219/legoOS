use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    auth_extractor::AuthUser,
    error::AppError,
    models::{DocumentResponse, DocumentRow},
    state::AppState,
    workspaces::{member_role, require_role},
};

const CHUNK_SIZE: usize = 1000;
const CHUNK_OVERLAP: usize = 200;

#[derive(Debug, Deserialize)]
pub struct CreateDocumentRequest {
    pub name: String,
    pub content: String,
}

pub async fn create_document(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<CreateDocumentRequest>,
) -> Result<Json<DocumentResponse>, AppError> {
    require_role(&state.pool, workspace_id, user_id, &["owner"]).await?;

    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::Validation(
            "document name must not be empty".into(),
        ));
    }
    let content = body.content.trim();
    if content.is_empty() {
        return Err(AppError::Validation(
            "document content must not be empty".into(),
        ));
    }

    let (document_id, created_at) = sqlx::query_as::<_, (Uuid, chrono::DateTime<chrono::Utc>)>(
        "INSERT INTO documents (workspace_id, name, content) VALUES ($1, $2, $3)
         RETURNING id, created_at",
    )
    .bind(workspace_id)
    .bind(name)
    .bind(content)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    // Ingested in the background rather than via the durable workflow queue: unlike a
    // workflow run, nothing blocks on this finishing and losing it on a process restart
    // just means "re-upload" — not worth the extra queue plumbing for that.
    let pool = state.pool.clone();
    let rag_client = state.rag_client.clone();
    let embedding_provider = state.embedding_provider.clone();
    let content_owned = content.to_string();
    tokio::spawn(async move {
        let Some(embedding_provider) = embedding_provider else {
            tracing::error!(document_id = %document_id, "no embedding provider configured, cannot ingest document");
            let _ = sqlx::query(
                "UPDATE documents SET status = 'failed', error = 'no embedding provider configured' WHERE id = $1",
            )
            .bind(document_id)
            .execute(&pool)
            .await;
            return;
        };
        ingest_document(
            &pool,
            &rag_client,
            embedding_provider.as_ref(),
            document_id,
            workspace_id,
            &content_owned,
        )
        .await;
    });

    Ok(Json(DocumentResponse {
        id: document_id,
        name: name.to_string(),
        status: "pending".to_string(),
        error: None,
        created_at,
    }))
}

pub async fn list_documents(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Vec<DocumentResponse>>, AppError> {
    member_role(&state.pool, workspace_id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("workspace not found".into()))?;

    let rows = sqlx::query_as::<_, DocumentRow>(
        "SELECT id, name, status, error, created_at FROM documents
         WHERE workspace_id = $1 ORDER BY created_at",
    )
    .bind(workspace_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?;

    Ok(Json(rows.into_iter().map(Into::into).collect()))
}

pub async fn delete_document(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
    Path((workspace_id, document_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    require_role(&state.pool, workspace_id, user_id, &["owner"]).await?;

    let result = sqlx::query("DELETE FROM documents WHERE id = $1 AND workspace_id = $2")
        .bind(document_id)
        .bind(workspace_id)
        .execute(&state.pool)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("document not found".into()));
    }

    if let Err(e) = state
        .rag_client
        .delete_document(workspace_id, document_id)
        .await
    {
        // The document row is already gone; a stray Qdrant entry can't be reached again
        // (every search/RAG query is scoped by workspace_id + a document_id that no
        // longer exists in Postgres), so this is worth logging but not failing on.
        tracing::warn!(document_id = %document_id, error = %e, "failed to delete document's chunks from Qdrant");
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Chunks `content`, embeds each chunk, and upserts them into Qdrant, updating the
/// document's status to `ready` or `failed` (with `error` set) when done. Always resolves
/// to a terminal status — never leaves a document stuck at `pending`.
pub async fn ingest_document(
    pool: &PgPool,
    rag_client: &rag::RagClient,
    embedding_provider: &dyn llm::EmbeddingProvider,
    document_id: Uuid,
    workspace_id: Uuid,
    content: &str,
) {
    match try_ingest(
        pool,
        rag_client,
        embedding_provider,
        document_id,
        workspace_id,
        content,
    )
    .await
    {
        Ok(()) => {
            let _ =
                sqlx::query("UPDATE documents SET status = 'ready', error = NULL WHERE id = $1")
                    .bind(document_id)
                    .execute(pool)
                    .await;
        }
        Err(e) => {
            tracing::error!(document_id = %document_id, error = %e, "document ingestion failed");
            let _ = sqlx::query("UPDATE documents SET status = 'failed', error = $2 WHERE id = $1")
                .bind(document_id)
                .bind(e.to_string())
                .execute(pool)
                .await;
        }
    }
}

async fn try_ingest(
    _pool: &PgPool,
    rag_client: &rag::RagClient,
    embedding_provider: &dyn llm::EmbeddingProvider,
    document_id: Uuid,
    workspace_id: Uuid,
    content: &str,
) -> anyhow::Result<()> {
    let chunks = rag::chunk_text(content, CHUNK_SIZE, CHUNK_OVERLAP);
    if chunks.is_empty() {
        return Ok(());
    }

    let mut points = Vec::with_capacity(chunks.len());
    let mut vector_size = 0u64;
    for (index, chunk) in chunks.into_iter().enumerate() {
        let vector = embedding_provider
            .embed(&chunk)
            .await
            .map_err(|e| anyhow::anyhow!("embedding failed: {e}"))?;
        vector_size = vector.len() as u64;
        points.push(rag::ChunkPoint {
            id: Uuid::new_v4(),
            vector,
            workspace_id,
            document_id,
            chunk_index: index as i64,
            text: chunk,
        });
    }

    rag_client
        .ensure_collection(vector_size)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    rag_client
        .upsert_chunks(points)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    Ok(())
}
