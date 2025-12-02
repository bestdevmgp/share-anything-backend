use rand::Rng;

pub fn generate_share_code() -> String {
    let mut rng = rand::thread_rng();
    let code: u32 = rng.gen_range(100000..=999999);
    code.to_string()
}

pub fn parse_device_platform(user_agent: &str) -> String {
    match woothee::parser::Parser::new().parse(user_agent) {
        Some(result) => {
            let os = if result.os.is_empty() { "Unknown" } else { result.os };
            let name = if result.name.is_empty() { "Unknown" } else { result.name };
            format!("{} ({})", os, name)
        }
        None => "Unknown".to_string(),
    }
}