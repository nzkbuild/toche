use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub upstream_url: String,
    pub auth_method: AuthMethod,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub models: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuthMethod {
    #[serde(rename = "api_key")]
    ApiKey {
        header_name: String,
        key: String,
    },
    #[serde(rename = "bearer")]
    BearerToken {
        token: String,
    },
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profiles {
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}
