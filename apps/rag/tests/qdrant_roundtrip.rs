use rag::{ChunkPoint, MemoryEntry, RagClient};
use uuid::Uuid;

fn qdrant_url() -> String {
    std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://127.0.0.1:6334".to_string())
}

/// Exercises the real Qdrant wire protocol: collection creation, upsert, workspace-scoped
/// search, and document deletion. Needs a live Qdrant instance (see docker-compose.yml /
/// CI's `qdrant` service) — there is no in-process fake for the gRPC API.
#[tokio::test]
async fn upserts_and_finds_only_the_matching_workspace() -> anyhow::Result<()> {
    let client = RagClient::connect(&qdrant_url())?;
    client.ensure_collection(4).await?;

    let workspace_a = Uuid::new_v4();
    let workspace_b = Uuid::new_v4();
    let document_a = Uuid::new_v4();
    let document_b = Uuid::new_v4();

    client
        .upsert_chunks(vec![
            ChunkPoint {
                id: Uuid::new_v4(),
                vector: vec![1.0, 0.0, 0.0, 0.0],
                workspace_id: workspace_a,
                document_id: document_a,
                chunk_index: 0,
                text: "the sky is blue".to_string(),
            },
            ChunkPoint {
                id: Uuid::new_v4(),
                vector: vec![1.0, 0.0, 0.0, 0.0],
                workspace_id: workspace_b,
                document_id: document_b,
                chunk_index: 0,
                text: "should never be returned for workspace_a's search".to_string(),
            },
        ])
        .await?;

    // Give Qdrant a moment to index (default write consistency is immediate for a single
    // node, but a short retry loop keeps this robust against any scheduling jitter).
    let mut hits = Vec::new();
    for _ in 0..10 {
        hits = client
            .search(workspace_a, vec![1.0, 0.0, 0.0, 0.0], 10)
            .await?;
        if !hits.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].document_id, document_a);
    assert_eq!(hits[0].text, "the sky is blue");

    client.delete_document(workspace_a, document_a).await?;

    let mut hits_after_delete = client
        .search(workspace_a, vec![1.0, 0.0, 0.0, 0.0], 10)
        .await?;
    for _ in 0..10 {
        if hits_after_delete.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        hits_after_delete = client
            .search(workspace_a, vec![1.0, 0.0, 0.0, 0.0], 10)
            .await?;
    }
    assert!(hits_after_delete.is_empty());

    Ok(())
}

/// Memories live in their own collection, keyed by `agent_key` rather than `document_id` —
/// two agents in the same workspace shouldn't see each other's memories.
#[tokio::test]
async fn remembers_and_recalls_only_the_matching_agent_key() -> anyhow::Result<()> {
    let client = RagClient::connect(&qdrant_url())?;
    client.ensure_memories_collection(4).await?;

    let workspace_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    client
        .remember(MemoryEntry {
            id: Uuid::new_v4(),
            vector: vec![1.0, 0.0, 0.0, 0.0],
            workspace_id,
            agent_key: "billing-agent".to_string(),
            text: "the customer's plan is Pro".to_string(),
            created_at: now,
        })
        .await?;
    client
        .remember(MemoryEntry {
            id: Uuid::new_v4(),
            vector: vec![1.0, 0.0, 0.0, 0.0],
            workspace_id,
            agent_key: "support-agent".to_string(),
            text: "should never surface for billing-agent's recall".to_string(),
            created_at: now,
        })
        .await?;

    let mut hits = Vec::new();
    for _ in 0..10 {
        hits = client
            .recall(workspace_id, "billing-agent", vec![1.0, 0.0, 0.0, 0.0], 10)
            .await?;
        if !hits.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].text, "the customer's plan is Pro");

    Ok(())
}
