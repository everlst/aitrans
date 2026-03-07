//! Hotword vocabulary management via DashScope REST API
//!
//! Uses Alibaba Cloud's VocabularyService for creating, listing, querying,
//! updating, and deleting hotword lists used with gummy-realtime-v1 model.
//!
//! API endpoint: POST https://dashscope.aliyuncs.com/api/v1/services/audio/asr/customization
//! Each operation is distinguished by the `action` field in the `input` object.

use serde::{Deserialize, Serialize};

const DASHSCOPE_API_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/services/audio/asr/customization";

/// A single hotword entry for Gummy model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotword {
    /// The hotword text
    pub text: String,
    /// Weight [1, 5], commonly 4
    pub weight: u32,
    /// Source language code (optional, model auto-detects if omitted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// Translation target language (required for Gummy models)
    pub target_lang: String,
    /// Expected translation result (required for Gummy models)
    pub translation: String,
}

/// Information about a vocabulary list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabularyInfo {
    pub vocabulary_id: String,
    #[serde(default)]
    pub gmt_create: String,
    #[serde(default)]
    pub gmt_modified: String,
    #[serde(default)]
    pub target_model: String,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub status: String,
}

/// Detailed vocabulary including hotword entries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabularyDetail {
    #[serde(default)]
    pub vocabulary: Vec<Hotword>,
    #[serde(default)]
    pub gmt_create: String,
    #[serde(default)]
    pub gmt_modified: String,
    #[serde(default)]
    pub target_model: String,
    #[serde(default)]
    pub status: String,
}

// ─────────── Request / Response types ───────────

#[derive(Serialize)]
struct DashScopeRequest {
    model: String,
    input: serde_json::Value,
}

#[derive(Deserialize, Debug)]
struct DashScopeResponse {
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    output: Option<serde_json::Value>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

// ─────────── Client ───────────

pub struct VocabularyClient;

impl VocabularyClient {
    /// Create a new hotword vocabulary list
    pub async fn create_vocabulary(
        api_key: &str,
        target_model: &str,
        prefix: &str,
        vocabulary: &[Hotword],
    ) -> Result<String, String> {
        let input = serde_json::json!({
            "action": "create_vocabulary",
            "target_model": target_model,
            "prefix": prefix,
            "vocabulary": vocabulary,
        });

        let resp = Self::call_api(api_key, &input).await?;

        resp.output
            .and_then(|o| o.get("vocabulary_id").and_then(|v| v.as_str().map(String::from)))
            .ok_or_else(|| "创建热词表失败：响应中未包含 vocabulary_id".to_string())
    }

    /// List all vocabulary lists
    pub async fn list_vocabularies(
        api_key: &str,
        prefix: Option<&str>,
        page_index: u32,
        page_size: u32,
    ) -> Result<Vec<VocabularyInfo>, String> {
        let mut input = serde_json::json!({
            "action": "list_vocabulary",
            "page_index": page_index,
            "page_size": page_size,
        });

        if let Some(pfx) = prefix {
            input["prefix"] = serde_json::json!(pfx);
        }

        let resp = Self::call_api(api_key, &input).await?;

        let output = resp.output.ok_or("查询热词表失败：响应为空")?;
        let list = output
            .get("vocabulary_list")
            .ok_or("查询热词表失败：响应中未包含 vocabulary_list")?;

        serde_json::from_value(list.clone())
            .map_err(|e| format!("解析热词表列表失败: {}", e))
    }

    /// Query a specific vocabulary by ID
    pub async fn query_vocabulary(
        api_key: &str,
        vocabulary_id: &str,
    ) -> Result<VocabularyDetail, String> {
        let input = serde_json::json!({
            "action": "query_vocabulary",
            "vocabulary_id": vocabulary_id,
        });

        let resp = Self::call_api(api_key, &input).await?;
        let output = resp.output.ok_or("查询热词表详情失败：响应为空")?;

        serde_json::from_value(output)
            .map_err(|e| format!("解析热词表详情失败: {}", e))
    }

    /// Update vocabulary (replace all hotwords)
    pub async fn update_vocabulary(
        api_key: &str,
        vocabulary_id: &str,
        vocabulary: &[Hotword],
    ) -> Result<(), String> {
        let input = serde_json::json!({
            "action": "update_vocabulary",
            "vocabulary_id": vocabulary_id,
            "vocabulary": vocabulary,
        });

        Self::call_api(api_key, &input).await?;
        Ok(())
    }

    /// Delete a vocabulary list
    pub async fn delete_vocabulary(
        api_key: &str,
        vocabulary_id: &str,
    ) -> Result<(), String> {
        let input = serde_json::json!({
            "action": "delete_vocabulary",
            "vocabulary_id": vocabulary_id,
        });

        Self::call_api(api_key, &input).await?;
        Ok(())
    }

    /// Internal: call the DashScope customization API
    async fn call_api(
        api_key: &str,
        input: &serde_json::Value,
    ) -> Result<DashScopeResponse, String> {
        let client = reqwest::Client::new();

        let body = DashScopeRequest {
            model: "speech-biasing".to_string(),
            input: input.clone(),
        };

        let response = client
            .post(DASHSCOPE_API_URL)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP 请求失败: {}", e))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| format!("读取响应失败: {}", e))?;

        log::debug!("DashScope vocabulary API response ({}): {}", status, text);

        if !status.is_success() {
            // Try to parse error message from response
            if let Ok(err_resp) = serde_json::from_str::<DashScopeResponse>(&text) {
                let code = err_resp.code.unwrap_or_default();
                let msg = err_resp.message.unwrap_or_else(|| "未知错误".into());
                return Err(format!("API 错误 [{}]: {}", code, msg));
            }
            return Err(format!("API 请求失败 (HTTP {}): {}", status, text));
        }

        let resp: DashScopeResponse =
            serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {}", e))?;

        // Check for API-level errors
        if let Some(ref code) = resp.code {
            if !code.is_empty() {
                let msg = resp.message.as_deref().unwrap_or("未知错误");
                return Err(format!("API 错误 [{}]: {}", code, msg));
            }
        }

        Ok(resp)
    }
}
