use std::sync::Arc;

use aws_sdk_dynamodb::Client as DynamoClient;
use redis::aio::ConnectionManager;

use crate::push::provider::PushProvider;

/// Shared application state passed to all Axum handlers via `State`.
#[derive(Clone)]
pub struct AppState {
    pub dynamo: DynamoClient,
    pub redis: ConnectionManager,
    /// Shared HTTP client with connection pooling — reused for all outbound
    /// requests (Google JWKS, FCM, etc.) to avoid per-request TLS handshakes.
    pub http_client: reqwest::Client,
    /// Push notification provider resolved at startup (FCM now, APNs in future).
    /// Handlers dispatch through this trait — no provider-specific code in handlers.
    pub push_provider: Arc<dyn PushProvider>,
    pub primary_table: String,
    pub google_client_id_android: String,
    pub google_client_id_web: String,
    #[allow(dead_code)]
    pub google_client_secret: String,
    #[allow(dead_code)]
    pub google_redirect_uri: String,
    pub google_jwks: std::sync::Arc<tokio::sync::RwLock<(u64, Option<jsonwebtoken::jwk::JwkSet>)>>,
}

impl AppState {
    pub async fn new() -> Self {
        tracing::info!("Initializing AWS DynamoDB client...");

        let region = std::env::var("AWS_REGION").expect("AWS_REGION must be set");

        // DynamoDB
        let aws_config = aws_config::from_env()
            .region(aws_sdk_dynamodb::config::Region::new(region))
            .load()
            .await;
        let dynamo = DynamoClient::new(&aws_config);

        tracing::info!("Initializing Redis connection manager...");
        // Redis
        let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
        let redis_client = redis::Client::open(redis_url).expect("Invalid REDIS_URL");

        // Wait for up to 5 seconds for the initial Redis connection
        let redis = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            ConnectionManager::new(redis_client),
        )
        .await
        .expect("Redis connection attempt timed out. Is Redis running?")
        .expect("Failed to create Redis connection manager");

        tracing::info!("Loading application configuration...");
        // Tables & OAuth config
        let primary_table = std::env::var("PRIMARY_TABLE").expect("PRIMARY_TABLE must be set");
        let google_client_id_android =
            std::env::var("GOOGLE_CLIENT_ID_ANDROID").unwrap_or_default();
        let google_client_id_web =
            std::env::var("GOOGLE_CLIENT_ID_WEB").unwrap_or_default();
        let google_client_secret = std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_default();
        let google_redirect_uri = std::env::var("GOOGLE_REDIRECT_URI").unwrap_or_default();

        tracing::info!("Initializing shared HTTP client with timeouts...");
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        tracing::info!("Initializing push notification provider...");
        let push_provider: Arc<dyn PushProvider> =
            Arc::new(crate::push::fcm::FcmProvider::from_env(http_client.clone()));

        Self {
            dynamo,
            redis,
            http_client,
            push_provider,
            primary_table,
            google_client_id_android,
            google_client_id_web,
            google_client_secret,
            google_redirect_uri,
            google_jwks: std::sync::Arc::new(tokio::sync::RwLock::new((0, None))),
        }
    }
}
