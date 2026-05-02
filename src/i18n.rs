use std::collections::HashMap;

pub struct I18n {
    messages: HashMap<String, String>,
}

impl I18n {
    pub fn load(locale: &str) -> Self {
        let raw = match locale {
            "en-US" => include_str!("../app/resources/locale/en-US/app.toml"),
            _ => include_str!("../app/resources/locale/zh-CN/app.toml"),
        };

        let messages = toml::from_str(raw).unwrap_or_default();
        Self { messages }
    }

    pub fn t(&self, key: &str) -> String {
        self.messages
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }
}
