use sqlx::postgres::PgPoolOptions;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use borsh::BorshDeserialize;
use tokio::time::{sleep , Duration};

#[derive(BorshDeserialize, Debug)]
pub struct NetworkStats {
    pub total_nodes: u64,
}

#[derive(BorshDeserialize, Debug)]
pub struct NodeDevice {
    pub authority: Pubkey,
    pub uri: String,
}

fn skip_anchor_discriminator(data: &[u8]) -> &[u8] {
    &data[8..] // skip first 8 bytes (Anchor discriminator)
}

fn deserialize_node_device(data: &[u8]) -> Result<NodeDevice, Box<dyn std::error::Error>> {
    let mut slice = skip_anchor_discriminator(data);

    // Authority is always 32 bytes
    let authority_bytes: [u8; 32] = slice[0..32].try_into()?;
    let authority = Pubkey::new_from_array(authority_bytes);

    slice = &slice[32..];

    // Next 4 bytes: length of the string
    let uri_len = u32::from_le_bytes(slice[0..4].try_into()?) as usize;
    slice = &slice[4..];

    // The actual URI string
    let uri = String::from_utf8(slice[0..uri_len].to_vec())?;

    Ok(NodeDevice { authority, uri })
}

fn deserialize_network_stats(data: &[u8]) -> Result<NetworkStats, Box<dyn std::error::Error>> {
    let stats = NetworkStats::try_from_slice(skip_anchor_discriminator(data))?;
    Ok(stats)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    loop{
        println!("\n🔄 Polling Solana program accounts...");

        if let Err(e) = fetch_program_accounts(rpc_url, program_id, &pool).await {
            eprintln!("⚠️ Error during fetch: {}", e);
        }
        println!("✅ Polling cycle complete. Sleeping for 10 seconds...\n");

        sleep(Duration::from_secs(10)).await;
    }
}

async fn fetch_program_accounts(
    rpc_url: &str,
    program_id: &str,
    pool: &sqlx::PgPool,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = RpcClient::new(rpc_url.to_string());
    let program_pubkey = Pubkey::from_str(program_id)?;

    let accounts = client.get_program_accounts(&program_pubkey)?;
    println!("Found {} accounts for program {}", accounts.len(), program_id);

    for (pubkey, account) in accounts {
        println!("Account pubkey: {}", pubkey);

        let data_len = account.data.len();

        if data_len == 8 + 32 + 4 + 256 || data_len > 40 {
            // NodeDevice accounts
            if let Ok(node) = deserialize_node_device(&account.data) {
                println!("NodeDevice: {:?}", node);

                sqlx::query(
                    r#"
                    INSERT INTO nodes (pubkey, authority, uri)
                    VALUES ($1, $2, $3)
                    ON CONFLICT (pubkey) DO UPDATE
                    SET authority = EXCLUDED.authority,
                        uri = EXCLUDED.uri
                    "#
                )
                .bind(pubkey.to_string())
                .bind(node.authority.to_string())
                .bind(node.uri)
                .execute(pool)
                .await?;
            } else {
                println!("Failed to deserialize NodeDevice for account {}", pubkey);
            }
        } else if data_len == 8 + 8 {
            // NetworkStats accounts
            if let Ok(_stats) = deserialize_network_stats(&account.data) {
                // Instead of using on-chain "total_nodes", count from DB
                let total_nodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
                    .fetch_one(pool)
                    .await?;

                println!("NetworkStats (calculated from DB): total_nodes={}", total_nodes);

                sqlx::query(
                    r#"
                    INSERT INTO network_stats (id, total_nodes)
                    VALUES (1, $1)
                    ON CONFLICT (id) DO UPDATE
                    SET total_nodes = EXCLUDED.total_nodes
                    "#
                )
                .bind(total_nodes)
                .execute(pool)
                .await?;
            } else {
                println!("Failed to deserialize NetworkStats for account {}", pubkey);
            }
        } else {
            println!("Unknown account type, raw size: {}", data_len);
        }
    }

    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM nodes")
        .fetch_one(pool)
        .await?;

    let total_nodes = row.0;

    sqlx::query(
        r#"
        INSERT INTO network_stats (id, total_nodes)
        VALUES (1, $1)
        ON CONFLICT (id) DO UPDATE
        SET total_nodes = EXCLUDED.total_nodes
        "#
    )
    .bind(total_nodes)
    .execute(pool)
    .await?;

    Ok(())
}