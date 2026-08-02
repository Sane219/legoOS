mod common {
    use redis::aio::ConnectionManager;
    pub async fn redis_conn() -> ConnectionManager {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        let client = redis::Client::open(redis_url.as_str()).expect("invalid REDIS_URL");
        ConnectionManager::new(client)
            .await
            .expect("failed to connect to redis for tests")
    }
}

use common::redis_conn;
use sqlx::PgPool;
use uuid::Uuid;

/// Not run by default — `cargo test` and CI's normal `cargo test --all` skip `#[ignore]`d
/// tests. Run explicitly: `cargo test -p worker --test load_test -- --ignored --nocapture`
/// (see `.github/workflows/load-test.yml` for a CI job that does exactly this, against real
/// Postgres/Redis/Qdrant service containers — a shared GitHub Actions runner, not production
/// hardware, so treat the numbers as a floor, not a ceiling; rerun on real infra before
/// trusting them for capacity planning). Uses only `input`/`transform` nodes so it needs no
/// LLM/embedding credentials.
#[sqlx::test]
#[ignore]
async fn load_test_reports_job_throughput(pool: PgPool) {
    const JOB_COUNT: usize = 500;

    let workspace_id: Uuid =
        sqlx::query_scalar("INSERT INTO workspaces (name) VALUES ('Load Test') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let workflow_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflows (workspace_id, name) VALUES ($1, 'load-test-workflow') RETURNING id",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let input_node: Uuid = sqlx::query_scalar(
        "INSERT INTO workflow_nodes (workflow_id, node_type, config) VALUES ($1, 'input', $2) RETURNING id",
    )
    .bind(workflow_id)
    .bind(serde_json::json!({ "value": { "n": 1 } }))
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut upstream = input_node;
    for i in 0..4 {
        let mut merge = serde_json::Map::new();
        merge.insert(format!("step_{i}"), serde_json::json!(i));
        let transform_node: Uuid = sqlx::query_scalar(
            "INSERT INTO workflow_nodes (workflow_id, node_type, config) VALUES ($1, 'transform', $2) RETURNING id",
        )
        .bind(workflow_id)
        .bind(serde_json::json!({ "merge": merge }))
        .fetch_one(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO workflow_edges (workflow_id, source_node_id, target_node_id) VALUES ($1, $2, $3)",
        )
        .bind(workflow_id)
        .bind(upstream)
        .bind(transform_node)
        .execute(&pool)
        .await
        .unwrap();

        upstream = transform_node;
    }

    let mut redis = redis_conn().await;
    worker::ensure_group(&mut redis).await.unwrap();
    let consumer = format!("load-test-{}", std::process::id());

    for _ in 0..JOB_COUNT {
        let execution_id: Uuid = sqlx::query_scalar(
            "INSERT INTO workflow_executions (workflow_id, status) VALUES ($1, 'pending') RETURNING id",
        )
        .bind(workflow_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        let job = queue::RunJob {
            execution_id,
            workflow_id,
        };
        let job_json = serde_json::to_string(&job).unwrap();
        let _: String = redis::AsyncCommands::xadd(
            &mut redis,
            queue::WORKFLOW_RUNS_STREAM,
            "*",
            &[(queue::JOB_FIELD, job_json)],
        )
        .await
        .unwrap();
    }

    let started_at = std::time::Instant::now();
    let mut processed = 0usize;
    while processed < JOB_COUNT {
        let entries = worker::read_new(&mut redis, &consumer, 2000).await.unwrap();
        if entries.is_empty() {
            break;
        }
        for (entry_id, job) in entries {
            worker::process_entry(&pool, &mut redis, &entry_id, job, None, "", None, None).await;
            processed += 1;
        }
    }
    let elapsed = started_at.elapsed();

    let succeeded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_executions WHERE workflow_id = $1 AND status = 'succeeded'",
    )
    .bind(workflow_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let jobs_per_sec = processed as f64 / elapsed.as_secs_f64();
    println!(
        "load test: {processed}/{JOB_COUNT} jobs processed ({succeeded} succeeded) in {:.2}s — {:.1} jobs/sec (5 nodes/job = {:.1} node executions/sec)",
        elapsed.as_secs_f64(),
        jobs_per_sec,
        jobs_per_sec * 5.0,
    );

    assert_eq!(
        processed, JOB_COUNT,
        "every enqueued job should be picked up"
    );
    assert_eq!(succeeded, JOB_COUNT as i64, "every job should succeed");
}
