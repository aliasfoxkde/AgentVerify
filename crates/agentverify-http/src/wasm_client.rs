//! WASM-native HTTP client using JavaScript fetch API
//!
//! This module provides HTTP functionality for wasm32 targets using
//! web-sys and wasm-bindgen to interface with JavaScript's fetch API.

use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, Response};

/// Error types for WASM HTTP operations
#[derive(Debug, thiserror::Error)]
pub enum WasmHttpError {
    #[error("Request failed: {0}")]
    RequestFailed(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    #[error("HTTP error: {status} - {message}")]
    HttpError { status: u16, message: String },
}

/// Fetch options for WASM HTTP client
#[derive(Debug, Clone, Default)]
pub struct WasmFetchOptions {
    pub headers: HashMap<String, String>,
    pub timeout_ms: Option<u32>,
}

/// WASM-native HTTP client using JavaScript fetch
#[derive(Clone)]
pub struct WasmHttpClient {
    base_url: String,
    options: WasmFetchOptions,
}

impl WasmHttpClient {
    /// Create a new WASM HTTP client
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            options: WasmFetchOptions::default(),
        }
    }

    /// Set custom headers
    #[must_use]
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.options.headers = headers;
        self
    }

    /// Set timeout in milliseconds
    #[must_use]
    pub fn with_timeout(mut self, timeout_ms: u32) -> Self {
        self.options.timeout_ms = Some(timeout_ms);
        self
    }

    /// Perform GET request and parse JSON response
    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, WasmHttpError> {
        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let text = self.fetch_text("GET", &url, None).await?;
        serde_json::from_str(&text).map_err(|e| WasmHttpError::InvalidResponse(e.to_string()))
    }

    /// Perform POST request with JSON body and parse JSON response
    pub async fn post_json<T: serde::Serialize + serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<T, WasmHttpError> {
        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let json = serde_json::to_string(body)
            .map_err(|e| WasmHttpError::InvalidResponse(e.to_string()))?;
        let text = self.fetch_text("POST", &url, Some(&json)).await?;
        serde_json::from_str(&text).map_err(|e| WasmHttpError::InvalidResponse(e.to_string()))
    }

    /// Perform a raw fetch and return text body
    async fn fetch_text(
        &self,
        method: &str,
        url: &str,
        body: Option<&str>,
    ) -> Result<String, WasmHttpError> {
        let opts = RequestInit::new();
        let _ = opts.set_method(method);

        if let Some(body_content) = body {
            let _ = opts.set_body(&JsValue::from_str(body_content));
        }

        let request = Request::new_with_str_and_init(url, &opts)
            .map_err(|e| WasmHttpError::RequestFailed(format!("Invalid request: {:?}", e)))?;

        // Get window and call fetch_with_request which returns a Promise<Response>
        let window = web_sys::Window::from(JsValue::from(js_sys::global()));

        let fetch_promise = window.fetch_with_request(&request);

        // JsFuture::from takes a Promise and converts it to a Future
        let resp_value = JsFuture::from(fetch_promise)
            .await
            .map_err(|e| WasmHttpError::RequestFailed(format!("Fetch failed: {:?}", e)))?;

        let response: Response = Response::from(resp_value);

        let status = response.status();
        let body_text_promise = response
            .text()
            .map_err(|e| WasmHttpError::RequestFailed(format!("Body error: {:?}", e)))?;
        let body = JsFuture::from(body_text_promise)
            .await
            .map_err(|e| WasmHttpError::RequestFailed(format!("Body text failed: {:?}", e)))?
            .as_string()
            .unwrap_or_default();

        if status < 200 || status >= 300 {
            return Err(WasmHttpError::HttpError {
                status,
                message: body.clone(),
            });
        }

        Ok(body)
    }
}
