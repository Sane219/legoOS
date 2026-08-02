use executor::{Edge, Node, RagContext, execute};
use rag::{ChunkPoint, RagClient};
use uuid::Uuid;

fn qdrant_url() -> String {
    std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://127.0.0.1:6334".to_string())
}

/// Embeds any text to the same fixed vector, so a query always "matches" whatever was
/// upserted with that same vector — deterministic without a real embedding model.
struct FixedVectorEmbeddingProvider;

#[async_trait::async_trait]
impl llm::EmbeddingProvider for FixedVectorEmbeddingProvider {
    async fn embed(&self, _text: &str) -> Result<Vec<f32>, llm::LlmError> {
        Ok(vec![1.0, 0.0, 0.0, 0.0])
    }

    fn name(&self) -> &'static str {
        "fixed"
    }
}

/// A `rag` node should embed its (template-rendered) query, search the workspace's
/// knowledge base in a real Qdrant, and splice the retrieved chunks into its output for a
/// downstream node's prompt template to pick up.
#[tokio::test]
async fn rag_node_retrieves_chunks_scoped_to_its_workspace() {
    let rag_client = RagClient::connect(&qdrant_url()).unwrap();
    rag_client.ensure_collection(4).await.unwrap();

    let workspace_id = Uuid::new_v4();
    let other_workspace_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();

    rag_client
        .upsert_chunks(vec![
            ChunkPoint {
                id: Uuid::new_v4(),
                vector: vec![1.0, 0.0, 0.0, 0.0],
                workspace_id,
                document_id,
                chunk_index: 0,
                text: "legoOS is a workflow automation platform".to_string(),
            },
            ChunkPoint {
                id: Uuid::new_v4(),
                vector: vec![1.0, 0.0, 0.0, 0.0],
                workspace_id: other_workspace_id,
                document_id: Uuid::new_v4(),
                chunk_index: 0,
                text: "this belongs to a different workspace".to_string(),
            },
        ])
        .await
        .unwrap();

    let input = Uuid::new_v4();
    let rag_node = Uuid::new_v4();
    let nodes = vec![
        Node {
            id: input,
            node_type: "input".to_string(),
            config: serde_json::json!({ "value": { "topic": "legoOS" } }),
        },
        Node {
            id: rag_node,
            node_type: "rag".to_string(),
            config: serde_json::json!({ "query": "what is {{topic}}?", "limit": 5 }),
        },
    ];
    let edges = vec![Edge {
        source: input,
        target: rag_node,
        condition: None,
    }];

    let embedding_provider = FixedVectorEmbeddingProvider;
    let rag_context = RagContext {
        client: &rag_client,
        embedding_provider: &embedding_provider,
        workspace_id,
    };

    // Retry briefly: the write above may not be searchable the instant it returns.
    let mut result = execute(&nodes, &edges, None, None, None, Some(&rag_context)).await;
    for _ in 0..10 {
        let output = result
            .nodes
            .iter()
            .find(|n| n.node_id == rag_node)
            .and_then(|n| n.output.clone());
        if output
            .as_ref()
            .and_then(|o| o.get("chunks"))
            .and_then(|c| c.as_array())
            .is_some_and(|arr| !arr.is_empty())
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        result = execute(&nodes, &edges, None, None, None, Some(&rag_context)).await;
    }

    let rag_result = result.nodes.iter().find(|n| n.node_id == rag_node).unwrap();
    assert_eq!(rag_result.status, executor::NodeStatus::Succeeded);
    let chunks = rag_result.output.as_ref().unwrap()["chunks"]
        .as_array()
        .unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(
        chunks[0]["text"],
        "legoOS is a workflow automation platform"
    );
    assert_eq!(chunks[0]["document_id"], document_id.to_string());
}

#[tokio::test]
async fn rag_node_without_context_fails_clearly() {
    let node_id = Uuid::new_v4();
    let nodes = vec![Node {
        id: node_id,
        node_type: "rag".to_string(),
        config: serde_json::json!({ "query": "anything" }),
    }];

    let result = execute(&nodes, &[], None, None, None, None).await;

    assert_eq!(result.status, executor::ExecutionStatus::Failed);
    let node_result = result.nodes.iter().find(|n| n.node_id == node_id).unwrap();
    assert!(
        node_result
            .error
            .as_deref()
            .unwrap()
            .contains("no RAG context configured")
    );
}
