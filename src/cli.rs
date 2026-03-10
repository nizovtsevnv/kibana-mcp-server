use std::env;

pub enum Command {
    Stdio,
    Http,
    Version,
    Help,
}

pub fn parse_args() -> Command {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.len() {
        0 => Command::Stdio,
        1 => match args[0].as_str() {
            "--stdio" => Command::Stdio,
            "--http" => Command::Http,
            "--version" | "-V" => Command::Version,
            "--help" | "-h" => Command::Help,
            other => {
                eprintln!("Unknown argument: {other}");
                eprintln!();
                print_help();
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("Expected at most one argument");
            eprintln!();
            print_help();
            std::process::exit(1);
        }
    }
}

pub fn print_help() {
    let version = env!("CARGO_PKG_VERSION");
    eprintln!("kibana-mcp-server {version}");
    eprintln!("MCP server for accessing logs in Kibana/Elasticsearch");
    eprintln!();
    eprintln!("Usage: kibana-mcp-server [COMMAND]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  --stdio      Run in stdio mode (default)");
    eprintln!("  --http       Run in HTTP mode");
    eprintln!("  --version    Print version and exit");
    eprintln!("  --help       Print this help and exit");
    eprintln!();
    eprintln!("Environment variables:");
    eprintln!("  KIBANA_URL        Kibana or Elasticsearch base URL (required)");
    eprintln!("  KIBANA_USERNAME   Username for basic authentication");
    eprintln!("  KIBANA_PASSWORD   Password for basic authentication");
    eprintln!("  KIBANA_API_KEY    API key for Elasticsearch authentication");
    eprintln!("  KIBANA_INSECURE   Skip TLS verification (\"true\" or \"1\")");
    eprintln!();
    eprintln!("HTTP mode variables:");
    eprintln!("  MCP_HOST          Host to bind [default: 127.0.0.1]");
    eprintln!("  MCP_PORT          Port to bind [default: 8080]");
    eprintln!("  MCP_AUTH_TOKEN    Bearer token for HTTP authentication");
}

pub fn print_version() {
    println!("kibana-mcp-server {}", env!("CARGO_PKG_VERSION"));
}
