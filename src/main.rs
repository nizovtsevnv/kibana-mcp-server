use std::sync::Arc;

use tracing::{info, warn};

mod cli;
mod config;
mod http;
mod kibana;
mod mcp;
mod tools;

#[tokio::main]
async fn main() {
    let command = cli::parse_args();

    match command {
        cli::Command::Help => {
            cli::print_help();
            return;
        }
        cli::Command::Version => {
            cli::print_version();
            return;
        }
        _ => {}
    }

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    match command {
        cli::Command::Stdio => {
            let kibana_config = match config::KibanaConfig::from_env() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };

            if kibana_config.insecure {
                warn!("TLS certificate verification is disabled");
            }

            info!("Connecting to {}", kibana_config.url);

            let client = kibana::KibanaClient::new(
                &kibana_config.url,
                kibana_config.username.as_deref(),
                kibana_config.password.as_deref(),
                kibana_config.api_key.as_deref(),
                kibana_config.insecure,
            );
            let client = Arc::new(client);

            info!("Starting MCP server (stdio)");
            mcp::run_stdio_loop(client).await;
        }
        cli::Command::Http => {
            let server_config = match config::ServerConfig::from_env() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            let http_config = match config::HttpConfig::from_env() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };

            if server_config.insecure {
                warn!("TLS certificate verification is disabled");
            }

            info!("Base URL: {}", server_config.url);
            info!("Starting MCP server (HTTP)");

            http::run_http_server(
                &server_config.url,
                server_config.insecure,
                &http_config.host,
                http_config.port,
                http_config.auth_token,
            )
            .await;
        }
        cli::Command::Help | cli::Command::Version => unreachable!(),
    }
}
