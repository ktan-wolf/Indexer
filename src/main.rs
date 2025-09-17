use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use borsh::BorshDeserialize;
use serde::Serialize;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::str::FromStr;
use tokio::time::{sleep, Duration};
// V-- NEW --V: Import the CORS layer
use tower_http::cors::{Any, CorsLayer};


// --- Type alias for our thread-safe error type ---
type AppError = Box<dyn std::error::Error + Send + Sync>;


#[derive(BorshDeserialize, Debug)]
pub struct NetworkStats {
    pub total_nodes: u64,
}

#[derive(BorshDeserialize, Debug, Serialize)]
pub struct NodeDevice {
    pub authority: Pubkey,
    pub uri: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct ApiNode {
    pub pubkey: String,
    pub authority: String,
    pub uri: String,
}

async fn get_nodes(
    State(pool): State<PgPool>,
) -> Result<Json<Vec<ApiNode>>, (StatusCode, String)> {
    println!("=> GET /nodes - Fetching nodes from database...");

    let nodes = sqlx::query_as::<_, ApiNode>("SELECT pubkey, authority, uri FROM nodes")
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            eprintln!("🔥 Database query failed: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch nodes from database".to_string())
        })?;

    println!("<= GET /nodes - Responding with {} nodes.", nodes.len());
    Ok(Json(nodes))
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let rpc_url = "http://127.0.0.1:8899";
    let client = RpcClient::new(rpc_url.to_string());
    let program_id = "3je23jfTQJBkYTYhLCBjH2F9thAcaY9g7M7RYR92uhWu";
    let database_url = "postgres://aethernet_user:devender@localhost:5432/aethernet";

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;
    println!("✅ Successfully connected to the database!");

    let slot = client.get_slot()?;
    println!("✅ Connected to Solana! Current slot: {}", slot);

    let pool_clone = pool.clone();
    tokio::spawn(async move {
        loop {
            println!("\n🔄 [Background Task] Polling Solana program accounts...");
            if let Err(e) = fetch_program_accounts(rpc_url, program_id, &pool_clone).await {
                eprintln!("⚠️ [Background Task] Error during fetch: {}", e);
            }
            println!("✅ [Background Task] Polling cycle complete. Sleeping for 10 seconds...");
            sleep(Duration::from_secs(10)).await;
        }
    });

    // V-- NEW --V: Create a CORS layer
    // This configuration is permissive and suitable for local development.
    let cors = CorsLayer::new()
        .allow_origin(Any) // Allows requests from any origin
        .allow_methods(Any) // Allows any HTTP method (GET, POST, etc.)
        .allow_headers(Any); // Allows any HTTP headers

    let app = Router::new()
        .route("/nodes", get(get_nodes))
        .with_state(pool)
        .layer(cors); // V-- NEW --V: Apply the CORS layer to the router

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("\n🚀 API server listening on http://{}", listener.local_addr()?);
    println!("   Try accessing http://localhost:3000/nodes in your browser.");
    axum::serve(listener, app).await?;

    Ok(())
}


fn skip_anchor_discriminator(data: &[u8]) -> &[u8] {
    &data[8..]
}

fn deserialize_node_device(data: &[u8]) -> Result<NodeDevice, AppError> {
    let mut slice = skip_anchor_discriminator(data);
    let authority_bytes: [u8; 32] = slice[0..32].try_into()?;
    let authority = Pubkey::new_from_array(authority_bytes);
    slice = &slice[32..];
    let uri_len = u32::from_le_bytes(slice[0..4].try_into()?) as usize;
    slice = &slice[4..];
    let uri = String::from_utf8(slice[0..uri_len].to_vec())?;
    Ok(NodeDevice { authority, uri })
}

fn deserialize_network_stats(data: &[u8]) -> Result<NetworkStats, AppError> {
    let stats = NetworkStats::try_from_slice(skip_anchor_discriminator(data))?;
    Ok(stats)
}

async fn fetch_program_accounts(
    rpc_url: &str,
    program_id: &str,
    pool: &sqlx::PgPool,
) -> Result<(), AppError> {
    let client = RpcClient::new(rpc_url.to_string());
    let program_pubkey = Pubkey::from_str(program_id)?;

    let accounts = client.get_program_accounts(&program_pubkey)?;
    println!("[Background Task] Found {} accounts for program {}", accounts.len(), program_id);

    for (pubkey, account) in accounts {
        let data_len = account.data.len();

        if data_len > 40 {
            if let Ok(node) = deserialize_node_device(&account.data) {
                println!("[Background Task] Deserialized NodeDevice: {:?}", node.uri);
                sqlx::query(
                    r#"
                    INSERT INTO nodes (pubkey, authority, uri)
                    VALUES ($1, $2, $3)
                    ON CONFLICT (pubkey) DO UPDATE
                    SET authority = EXCLUDED.authority,
                        uri = EXCLUDED.uri
                    "#,
                )
                .bind(pubkey.to_string())
                .bind(node.authority.to_string())
                .bind(node.uri)
                .execute(pool)
                .await?;
            } else {
                println!("[Background Task] Failed to deserialize NodeDevice for account {}", pubkey);
            }
        } else if data_len == 8 + 8 {
            if let Ok(_stats) = deserialize_network_stats(&account.data) {
                // Logic is handled at the end of the function
            } else {
                println!("[Background Task] Failed to deserialize NetworkStats for account {}", pubkey);
            }
        } else {
            println!("[Background Task] Unknown account type, raw size: {}", data_len);
        }
    }
    
    let total_nodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
        .fetch_one(pool)
        .await?;

    println!("[Background Task] Updating network_stats.total_nodes to {}", total_nodes);

    sqlx::query(
        r#"
        INSERT INTO network_stats (id, total_nodes)
        VALUES (1, $1)
        ON CONFLICT (id) DO UPDATE
        SET total_nodes = EXCLUDED.total_nodes
        "#,
    )
    .bind(total_nodes)
    .execute(pool)
    .await?;

    Ok(())
}