use rand::Rng;

pub fn generate_share_code() -> String {
    let mut rng = rand::thread_rng();
    let code: u32 = rng.gen_range(100000..=999999);
    code.to_string()
}

pub fn parse_device_platform(user_agent: &str) -> String {
    if user_agent.starts_with("share-cli/") {
        let os_info = user_agent
            .find('(')
            .and_then(|start| user_agent.find(')').map(|end| user_agent[start + 1..end].trim()));

        return match os_info {
            Some(info) if !info.is_empty() => format!("{} (CLI)", info),
            _ => "Unknown (CLI)".to_string(),
        };
    }

    match woothee::parser::Parser::new().parse(user_agent) {
        Some(result) => {
            let os = if result.os.is_empty() { "Unknown" } else { result.os };
            let name = if result.name.is_empty() { "Unknown" } else { result.name };
            format!("{} ({})", os, name)
        }
        None => "Unknown".to_string(),
    }
}