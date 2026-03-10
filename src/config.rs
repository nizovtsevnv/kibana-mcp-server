use std::env;

pub struct KibanaConfig {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub api_key: Option<String>,
    pub insecure: bool,
}

pub struct HttpConfig {
    pub host: String,
    pub port: u16,
    pub auth_token: Option<String>,
}

fn env_opt(name: &str) -> Option<String> {
    env::var(name).ok().filter(|s| !s.is_empty())
}

impl KibanaConfig {
    pub fn from_env() -> Result<Self, String> {
        let url = env_opt("KIBANA_URL").ok_or("KIBANA_URL environment variable is required")?;

        let username = env_opt("KIBANA_USERNAME");
        let password = env_opt("KIBANA_PASSWORD");
        let api_key = env_opt("KIBANA_API_KEY");
        let insecure = env_opt("KIBANA_INSECURE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        if username.is_some() != password.is_some() {
            return Err(
                "Both KIBANA_USERNAME and KIBANA_PASSWORD must be provided together".to_string(),
            );
        }

        if api_key.is_some() && username.is_some() {
            return Err(
                "Cannot use both KIBANA_API_KEY and KIBANA_USERNAME/KIBANA_PASSWORD".to_string(),
            );
        }

        Ok(Self {
            url,
            username,
            password,
            api_key,
            insecure,
        })
    }
}

impl HttpConfig {
    pub fn from_env() -> Result<Self, String> {
        let host = env_opt("MCP_HOST").unwrap_or_else(|| "127.0.0.1".to_string());

        let port = match env_opt("MCP_PORT") {
            Some(s) => s
                .parse::<u16>()
                .map_err(|_| format!("Invalid MCP_PORT value: {s}"))?,
            None => 8080,
        };

        let auth_token = env_opt("MCP_AUTH_TOKEN");

        Ok(Self {
            host,
            port,
            auth_token,
        })
    }
}
