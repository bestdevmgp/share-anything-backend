use serde::Deserialize;
use std::env;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub jwt: JwtConfig,
    pub oauth: OAuthConfig,
    pub s3: S3Config,
    pub cors: CorsConfig,
    pub turnstile: TurnstileConfig,
    pub session_token: SessionTokenConfig,
    pub cloudflare_turn: CloudflareTurnConfig,
    pub discord: DiscordConfig,
    pub smtp: SmtpConfig,
    pub ipinfo_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscordConfig {
    pub webhook_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SmtpConfig {
    pub host: Option<String>,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from_email: Option<String>,
    pub from_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub base_url: String,
    pub frontend_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JwtConfig {
    pub secret: String,
    pub expiration_hours: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthConfig {
    pub google: OAuthProvider,
    pub naver: OAuthProvider,
    pub kakao: OAuthProvider,
    pub apple: AppleOAuthConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppleOAuthConfig {
    pub client_id: String,
    pub team_id: String,
    pub key_id: String,
    pub private_key: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OAuthProvider {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct S3Config {
    pub endpoint: Option<String>,
    pub region: String,
    pub bucket_name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub prefix: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TurnstileConfig {
    pub secret_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionTokenConfig {
    pub jwt_secret: String,
    pub ttl_seconds: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloudflareTurnConfig {
    pub key_id: String,
    pub api_token: String,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        dotenvy::dotenv().ok();

        let config = Config {
            server: ServerConfig {
                host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: env::var("SERVER_PORT")
                    .unwrap_or_else(|_| "8080".to_string())
                    .parse()?,
                base_url: env::var("BASE_URL")
                    .unwrap_or_else(|_| "http://localhost:8080".to_string()),
                frontend_url: env::var("FRONTEND_URL")
                    .unwrap_or_else(|_| "http://localhost:3000".to_string()),
            },
            database: DatabaseConfig {
                url: env::var("DATABASE_URL")
                    .expect("DATABASE_URL must be set in environment"),
            },
            jwt: JwtConfig {
                secret: env::var("JWT_SECRET")
                    .expect("JWT_SECRET must be set in environment"),
                expiration_hours: env::var("JWT_EXPIRATION_HOURS")
                    .unwrap_or_else(|_| "24".to_string())
                    .parse()?,
            },
            oauth: OAuthConfig {
                google: OAuthProvider {
                    client_id: env::var("GOOGLE_CLIENT_ID")
                        .expect("GOOGLE_CLIENT_ID must be set"),
                    client_secret: env::var("GOOGLE_CLIENT_SECRET")
                        .expect("GOOGLE_CLIENT_SECRET must be set"),
                    redirect_uri: env::var("GOOGLE_REDIRECT_URI")
                        .expect("GOOGLE_REDIRECT_URI must be set"),
                },
                naver: OAuthProvider {
                    client_id: env::var("NAVER_CLIENT_ID")
                        .expect("NAVER_CLIENT_ID must be set"),
                    client_secret: env::var("NAVER_CLIENT_SECRET")
                        .expect("NAVER_CLIENT_SECRET must be set"),
                    redirect_uri: env::var("NAVER_REDIRECT_URI")
                        .expect("NAVER_REDIRECT_URI must be set"),
                },
                kakao: OAuthProvider {
                    client_id: env::var("KAKAO_CLIENT_ID")
                        .expect("KAKAO_CLIENT_ID must be set"),
                    client_secret: env::var("KAKAO_CLIENT_SECRET")
                        .expect("KAKAO_CLIENT_SECRET must be set"),
                    redirect_uri: env::var("KAKAO_REDIRECT_URI")
                        .expect("KAKAO_REDIRECT_URI must be set"),
                },
                apple: AppleOAuthConfig {
                    client_id: env::var("APPLE_CLIENT_ID")
                        .expect("APPLE_CLIENT_ID must be set"),
                    team_id: env::var("APPLE_TEAM_ID")
                        .expect("APPLE_TEAM_ID must be set"),
                    key_id: env::var("APPLE_KEY_ID")
                        .expect("APPLE_KEY_ID must be set"),
                    private_key: env::var("APPLE_PRIVATE_KEY")
                        .expect("APPLE_PRIVATE_KEY must be set")
                        .replace("\\n", "\n"),
                    redirect_uri: env::var("APPLE_REDIRECT_URI")
                        .expect("APPLE_REDIRECT_URI must be set"),
                },
            },
            s3: S3Config {
                endpoint: env::var("S3_ENDPOINT").ok(),
                region: env::var("S3_REGION").unwrap_or_else(|_| "auto".to_string()),
                bucket_name: env::var("S3_BUCKET_NAME")
                    .expect("S3_BUCKET_NAME must be set"),
                access_key_id: env::var("S3_ACCESS_KEY_ID")
                    .expect("S3_ACCESS_KEY_ID must be set"),
                secret_access_key: env::var("S3_SECRET_ACCESS_KEY")
                    .expect("S3_SECRET_ACCESS_KEY must be set"),
                prefix: env::var("S3_PREFIX").unwrap_or_else(|_| "".to_string()),
            },
            cors: CorsConfig {
                allowed_origins: env::var("CORS_ALLOWED_ORIGINS")
                    .unwrap_or_else(|_| "http://localhost:3000".to_string())
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect(),
            },
            turnstile: TurnstileConfig {
                secret_key: env::var("TURNSTILE_SECRET_KEY")
                    .expect("TURNSTILE_SECRET_KEY must be set in environment"),
            },
            session_token: SessionTokenConfig {
                jwt_secret: env::var("SESSION_TOKEN_JWT_SECRET")
                    .expect("SESSION_TOKEN_JWT_SECRET must be set in environment"),
                ttl_seconds: env::var("SESSION_TOKEN_TTL_SECONDS")
                    .unwrap_or_else(|_| "1800".to_string())
                    .parse()
                    .expect("SESSION_TOKEN_TTL_SECONDS must be an integer"),
            },
            cloudflare_turn: CloudflareTurnConfig {
                key_id: env::var("CLOUDFLARE_TURN_KEY_ID")
                    .expect("CLOUDFLARE_TURN_KEY_ID must be set in environment"),
                api_token: env::var("CLOUDFLARE_TURN_API_TOKEN")
                    .expect("CLOUDFLARE_TURN_API_TOKEN must be set in environment"),
            },
            discord: DiscordConfig {
                webhook_url: env::var("DISCORD_WEBHOOK_URL").ok(),
            },
            smtp: SmtpConfig {
                host: env::var("SMTP_HOST").ok(),
                port: env::var("SMTP_PORT")
                    .unwrap_or_else(|_| "587".to_string())
                    .parse()?,
                username: env::var("SMTP_USERNAME").ok(),
                password: env::var("SMTP_PASSWORD").ok(),
                from_email: env::var("SMTP_FROM_EMAIL").ok(),
                from_name: env::var("SMTP_FROM_NAME").ok(),
            },
            ipinfo_token: env::var("IPINFO_TOKEN").ok(),
        };

        validate_url("BASE_URL", &config.server.base_url)?;
        validate_url("FRONTEND_URL", &config.server.frontend_url)?;
        validate_url("GOOGLE_REDIRECT_URI", &config.oauth.google.redirect_uri)?;
        validate_url("NAVER_REDIRECT_URI", &config.oauth.naver.redirect_uri)?;
        validate_url("KAKAO_REDIRECT_URI", &config.oauth.kakao.redirect_uri)?;
        validate_url("APPLE_REDIRECT_URI", &config.oauth.apple.redirect_uri)?;
        if let Some(url) = &config.discord.webhook_url {
            validate_url("DISCORD_WEBHOOK_URL", url)?;
        }
        if let Some(url) = &config.s3.endpoint {
            validate_url("S3_ENDPOINT", url)?;
        }

        Ok(config)
    }
}

fn validate_url(name: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    oauth2::url::Url::parse(value)
        .map_err(|e| format!("Invalid URL in {}: '{}' ({})", name, value, e))?;
    Ok(())
}
