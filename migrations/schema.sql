-- Table for storing NodeDevice accounts
CREATE TABLE IF NOT EXISTS nodes (
    pubkey TEXT PRIMARY KEY,
    authority TEXT NOT NULL,
    uri TEXT NOT NULL
);

-- Table for storing network stats
CREATE TABLE IF NOT EXISTS network_stats (
    id SERIAL PRIMARY KEY,
    total_nodes BIGINT NOT NULL
);
