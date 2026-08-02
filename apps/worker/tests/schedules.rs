use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use uuid::Uuid;

async fn redis_conn() -> ConnectionManager {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let client = redis::Client::open(redis_url.as_str()).expect("invalid REDIS_URL");
    ConnectionManager::new(client)
        .await
        .expect("failed to connect to redis for tests")
}

/// A due, enabled schedule should get a fresh pending execution, its `next_run_at` pushed
/// into the future, `last_run_at` stamped, and a job enqueued — while a schedule that
/// isn't due yet, or is disabled, is left untouched.
#[sqlx::test]
async fn run_due_schedules_fires_only_whats_actually_due(pool: PgPool) {
    let workspace_id: Uuid =
        sqlx::query_scalar("INSERT INTO workspaces (name) VALUES ('Acme') RETURNING id")
            .fetch_one(&pool)
            .await
            .unwrap();
    let workflow_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflows (workspace_id, name) VALUES ($1, 'wf') RETURNING id",
    )
    .bind(workspace_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let due_schedule_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflow_schedules (workflow_id, cron_expression, enabled, next_run_at)
         VALUES ($1, '0 9 * * *', true, now() - interval '1 minute') RETURNING id",
    )
    .bind(workflow_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let not_due_schedule_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflow_schedules (workflow_id, cron_expression, enabled, next_run_at)
         VALUES ($1, '0 9 * * *', true, now() + interval '1 day') RETURNING id",
    )
    .bind(workflow_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let disabled_schedule_id: Uuid = sqlx::query_scalar(
        "INSERT INTO workflow_schedules (workflow_id, cron_expression, enabled, next_run_at)
         VALUES ($1, '0 9 * * *', false, now() - interval '1 minute') RETURNING id",
    )
    .bind(workflow_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut redis = redis_conn().await;
    let stream_len_before: i64 = redis
        .clone()
        .xlen(queue::WORKFLOW_RUNS_STREAM)
        .await
        .unwrap_or(0);

    worker::run_due_schedules(&pool, &mut redis).await.unwrap();

    let stream_len_after: i64 = redis.xlen(queue::WORKFLOW_RUNS_STREAM).await.unwrap();
    assert_eq!(stream_len_after, stream_len_before + 1);

    let executions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_executions WHERE workflow_id = $1")
            .bind(workflow_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(executions, 1, "only the due schedule should have fired");

    let (next_run_at, last_run_at): (
        chrono::DateTime<chrono::Utc>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) = sqlx::query_as("SELECT next_run_at, last_run_at FROM workflow_schedules WHERE id = $1")
        .bind(due_schedule_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(next_run_at > chrono::Utc::now());
    assert!(last_run_at.is_some());

    let (not_due_next_run,): (chrono::DateTime<chrono::Utc>,) =
        sqlx::query_as("SELECT next_run_at FROM workflow_schedules WHERE id = $1")
            .bind(not_due_schedule_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(not_due_next_run > chrono::Utc::now() + chrono::Duration::hours(23));

    let (disabled_last_run,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT last_run_at FROM workflow_schedules WHERE id = $1")
            .bind(disabled_schedule_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(disabled_last_run.is_none());
}
