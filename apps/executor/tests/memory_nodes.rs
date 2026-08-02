use executor::{Edge, Node, RagContext, execute};
use rag::RagClient;
use uuid::Uuid;

fn qdrant_url() -> String {
    std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://127.0.0.1:6334".to_string())
}

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

/// A `memory_write` node should persist its rendered content for its `agent_key`, and a
/// later `memory_read` node (same agent_key, same workspace) should recall it — proving
/// the write -> retrieve round trip through a real Qdrant, not just that both node types
/// individually call into `rag` without erroring.
#[tokio::test]
async fn memory_written_by_one_run_is_recalled_by_a_later_read() {
    let rag_client = RagClient::connect(&qdrant_url()).unwrap();
    rag_client.ensure_memories_collection(4).await.unwrap();

    let workspace_id = Uuid::new_v4();
    let embedding_provider = FixedVectorEmbeddingProvider;
    let rag_context = RagContext {
        client: &rag_client,
        embedding_provider: &embedding_provider,
        workspace_id,
    };

    let input = Uuid::new_v4();
    let write_node = Uuid::new_v4();
    let write_nodes = vec![
        Node {
            id: input,
            node_type: "input".to_string(),
            config: serde_json::json!({ "value": { "fact": "the customer's plan is Pro" } }),
        },
        Node {
            id: write_node,
            node_type: "memory_write".to_string(),
            config: serde_json::json!({ "agent_key": "billing-agent", "content": "{{fact}}" }),
        },
    ];
    let write_edges = vec![Edge {
        source: input,
        target: write_node,
        condition: None,
    }];

    let write_result = execute(
        &write_nodes,
        &write_edges,
        None,
        None,
        None,
        Some(&rag_context),
    )
    .await;
    assert_eq!(write_result.status, executor::ExecutionStatus::Succeeded);

    let read_node = Uuid::new_v4();
    let read_nodes = vec![Node {
        id: read_node,
        node_type: "memory_read".to_string(),
        config: serde_json::json!({ "agent_key": "billing-agent", "query": "what plan?" }),
    }];

    let mut read_result = execute(&read_nodes, &[], None, None, None, Some(&rag_context)).await;
    for _ in 0..10 {
        let has_memories = read_result
            .nodes
            .iter()
            .find(|n| n.node_id == read_node)
            .and_then(|n| n.output.as_ref())
            .and_then(|o| o.get("memories"))
            .and_then(|m| m.as_array())
            .is_some_and(|arr| !arr.is_empty());
        if has_memories {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        read_result = execute(&read_nodes, &[], None, None, None, Some(&rag_context)).await;
    }

    let read_node_result = read_result
        .nodes
        .iter()
        .find(|n| n.node_id == read_node)
        .unwrap();
    assert_eq!(read_node_result.status, executor::NodeStatus::Succeeded);
    let memories = read_node_result.output.as_ref().unwrap()["memories"]
        .as_array()
        .unwrap();
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0]["text"], "the customer's plan is Pro");
}

#[tokio::test]
async fn memory_read_is_scoped_to_its_own_agent_key() {
    let rag_client = RagClient::connect(&qdrant_url()).unwrap();
    rag_client.ensure_memories_collection(4).await.unwrap();

    let workspace_id = Uuid::new_v4();
    let embedding_provider = FixedVectorEmbeddingProvider;
    let rag_context = RagContext {
        client: &rag_client,
        embedding_provider: &embedding_provider,
        workspace_id,
    };

    let write_node = Uuid::new_v4();
    let write_nodes = vec![Node {
        id: write_node,
        node_type: "memory_write".to_string(),
        config: serde_json::json!({
            "agent_key": "support-agent",
            "content": "should never surface for billing-agent's recall",
        }),
    }];
    execute(&write_nodes, &[], None, None, None, Some(&rag_context)).await;

    // Give Qdrant a moment, then confirm billing-agent's recall stays empty regardless.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let read_node = Uuid::new_v4();
    let read_nodes = vec![Node {
        id: read_node,
        node_type: "memory_read".to_string(),
        config: serde_json::json!({ "agent_key": "billing-agent", "query": "anything" }),
    }];
    let read_result = execute(&read_nodes, &[], None, None, None, Some(&rag_context)).await;

    let read_node_result = read_result
        .nodes
        .iter()
        .find(|n| n.node_id == read_node)
        .unwrap();
    let memories = read_node_result.output.as_ref().unwrap()["memories"]
        .as_array()
        .unwrap();
    assert!(memories.is_empty());
}
