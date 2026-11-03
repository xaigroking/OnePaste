//! Binary attachment requests never put file names or plaintext metadata in headers or paths.
use crate::{ApiClient, ClientError};
use onepaste_core::attachment::MAX_SEALED_CHUNK;
use onepaste_sync::{HttpRequest, HttpResponse, Method};

impl ApiClient {
    async fn attachment_request(
        &self,
        method: Method,
        path: String,
        mut extra: Vec<(String, String)>,
        body: Vec<u8>,
        max_response: usize,
    ) -> Result<HttpResponse, ClientError> {
        let mut headers = vec![
            ("OnePaste-Proto".into(), "1".into()),
            ("Authorization".into(), format!("Bearer {}", self.token)),
        ];
        headers.append(&mut extra);
        self.transport
            .send_bounded(
                &HttpRequest {
                    method,
                    path_and_query: path,
                    headers,
                    body,
                },
                max_response,
            )
            .await
    }
    pub async fn attachment_capabilities(&self) -> Result<HttpResponse, ClientError> {
        self.attachment_request(Method::Get, "/v1/capabilities".into(), vec![], vec![], 8192)
            .await
    }
    pub async fn attachment_manifest(&self, blob_id: &str) -> Result<HttpResponse, ClientError> {
        onepaste_core::id::validate_id(blob_id)?;
        self.attachment_request(
            Method::Get,
            format!("/v1/blobs/{blob_id}"),
            vec![],
            vec![],
            8192,
        )
        .await
    }
    pub async fn upload_attachment_chunk(
        &self,
        blob_id: &str,
        index: u32,
        chunks: u32,
        sealed: Vec<u8>,
    ) -> Result<HttpResponse, ClientError> {
        onepaste_core::id::validate_id(blob_id)?;
        if sealed.len() > MAX_SEALED_CHUNK {
            return Err(ClientError::Core(onepaste_core::Error::TooLarge));
        }
        let headers = vec![
            ("Idempotency-Key".into(), format!("{blob_id}-{index}")),
            ("Content-Type".into(), "application/octet-stream".into()),
            ("OnePaste-Blob-Version".into(), "1".into()),
            ("OnePaste-Blob-Chunks".into(), chunks.to_string()),
        ];
        self.attachment_request(
            Method::Put,
            format!("/v1/blobs/{blob_id}/{index}"),
            headers,
            sealed,
            8192,
        )
        .await
    }
    pub async fn download_attachment_chunk(
        &self,
        blob_id: &str,
        index: u32,
    ) -> Result<HttpResponse, ClientError> {
        onepaste_core::id::validate_id(blob_id)?;
        self.attachment_request(
            Method::Get,
            format!("/v1/blobs/{blob_id}/{index}"),
            vec![],
            vec![],
            MAX_SEALED_CHUNK,
        )
        .await
    }
}
