use std::sync::Arc;

use clap::Parser;
use tracing::{info, warn};

mod http;
mod kibana;
mod mcp;
mod tools;

#[derive(Parser)]
#[command(name = "kibana-mcp-server", version)]
struct Args {
    /// Kibana or Elasticsearch base URL
    #[arg(long)]
    kibana_url: String,

    /// Username for basic authentication
    #[arg(long)]
    username: Option<String>,

    /// Password for basic authentication
    #[arg(long)]
    password: Option<String>,

    /// API key for Elasticsearch authentication
    #[arg(long)]
    api_key: Option<String>,

    /// Skip TLS certificate verification
    #[arg(long)]
    insecure: bool,

    /// Transport mode: stdio or http
    #[arg(long, default_value = "stdio")]
    transport: String,

    /// Host to bind HTTP server (http transport only)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port for HTTP server (http transport only)
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Bearer token for HTTP authentication (http transport only)
    #[arg(long)]
    auth: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let args = Args::parse();

    // Validate auth args
    if args.username.is_some() != args.password.is_some() {
        eprintln!("Both --username and --password must be provided together");
        std::process::exit(1);
    }

    if args.insecure {
        warn!("TLS certificate verification is disabled");
    }

    if args.api_key.is_some() && args.username.is_some() {
        eprintln!("Cannot use both --api-key and --username/--password");
        std::process::exit(1);
    }

    info!("Connecting to {}", args.kibana_url);

    let client = kibana::KibanaClient::new(
        &args.kibana_url,
        args.username.as_deref(),
        args.password.as_deref(),
        args.api_key.as_deref(),
        args.insecure,
    );
    let client = Arc::new(client);

    info!("Starting MCP server");

    match args.transport.as_str() {
        "stdio" => mcp::run_stdio_loop(client).await,
        "http" => {
            http::run_http_server(client, &args.host, args.port, args.auth).await;
        }
        other => {
            eprintln!("Unknown transport: {other}");
            std::process::exit(1);
        }
    }
}
