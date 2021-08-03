use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cw20::TokenInfoResponse;

#[derive(Serialize, Deserialize, Clone, PartialEq, JsonSchema, Debug)]
pub struct TokenInfoResponseWithMeta {
    pub external_permalink_uri: String,
    pub artist: String,
    pub work: String,
    pub description: String,
    pub asset_uri: Option<String>,
    pub token_info_response: TokenInfoResponse,
}
