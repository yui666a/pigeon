use base64::Engine;
use serde::Serialize;

use crate::error::AppError;

/// 外部画像1枚の取得結果。data_uri はフロントで元URLと置換される。
#[derive(Debug, Serialize)]
pub struct FetchedImage {
    pub url: String,
    pub data_uri: String,
}

/// 1リクエストあたりの取得上限枚数（フロントの MAX_EXTERNAL_IMAGES と同値）。
const MAX_IMAGES: usize = 20;
/// 1枚あたりのサイズ上限（メモリ枯渇防止）。
const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// 1枚あたりのタイムアウト。
const FETCH_TIMEOUT_SECS: u64 = 10;
/// リダイレクトの上限ホップ数。
const MAX_REDIRECTS: usize = 5;

/// 取得を許可する外部画像URLか検証する（設計書 2026-07-15-external-image-optin-design.md）。
/// http(s) 以外のスキームと、IPリテラルの private/loopback/link-local、localhost を拒否し、
/// メール由来URLによる内部ネットワークのプローブを防ぐ。
fn validate_image_url(url_str: &str) -> Result<reqwest::Url, AppError> {
    let url = reqwest::Url::parse(url_str)
        .map_err(|e| AppError::Validation(format!("invalid image url: {e}")))?;

    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(AppError::Validation(format!(
            "image url scheme not allowed: {}",
            url.scheme()
        )));
    }

    let host = url
        .host()
        .ok_or_else(|| AppError::Validation("image url has no host".into()))?;
    match host {
        url::Host::Ipv4(ip) => {
            if !is_public_ipv4(ip) {
                return Err(AppError::Validation(format!(
                    "image url points to non-public address: {ip}"
                )));
            }
        }
        url::Host::Ipv6(ip) => {
            if ip.is_loopback() || ip.is_unspecified() {
                return Err(AppError::Validation(format!(
                    "image url points to non-public address: {ip}"
                )));
            }
        }
        url::Host::Domain(domain) => {
            if domain.eq_ignore_ascii_case("localhost") {
                return Err(AppError::Validation("image url points to localhost".into()));
            }
        }
    }
    Ok(url)
}

/// IPv4 が公開アドレスか（private/loopback/link-local/unspecified/broadcast を除外）。
fn is_public_ipv4(ip: std::net::Ipv4Addr) -> bool {
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast())
}

/// Content-Type が data URI に安全に埋め込める画像型か検証する。
/// `image/` + 英数と `.+-` のみを許可し、非画像型と区切り文字の注入を防ぐ。
fn validate_image_content_type(content_type: &str) -> Result<String, AppError> {
    // "image/png; charset=..." のようなパラメータ部は捨てる
    let mime = content_type.split(';').next().unwrap_or("").trim();
    let Some(subtype) = mime.strip_prefix("image/") else {
        return Err(AppError::Validation(format!(
            "not an image content type: {content_type}"
        )));
    };
    if subtype.is_empty()
        || !subtype
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'+' || b == b'-')
    {
        return Err(AppError::Validation(format!(
            "suspicious image content type: {content_type}"
        )));
    }
    Ok(mime.to_string())
}

/// 検証済み Content-Type と画像バイト列から data URI を組み立てる。
fn build_data_uri(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// 外部画像取得用の HTTP クライアント。リダイレクトの各ホップにも
/// validate_image_url を通し、検証済みURL→内部アドレスへの迂回を防ぐ。
fn build_image_client() -> Result<reqwest::Client, AppError> {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        match validate_image_url(attempt.url().as_str()) {
            Ok(_) => attempt.follow(),
            Err(_) => attempt.stop(),
        }
    });
    reqwest::Client::builder()
        .redirect(policy)
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| AppError::HttpRequest(format!("failed to build http client: {e}")))
}

/// 1枚取得して data URI 化する。検証・上限のいずれかに反したらエラー。
async fn fetch_one(client: &reqwest::Client, url_str: &str) -> Result<FetchedImage, AppError> {
    let url = validate_image_url(url_str)?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::HttpRequest(format!("failed to fetch image: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::HttpRequest(format!(
            "image fetch returned status {}",
            resp.status()
        )));
    }

    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mime = validate_image_content_type(&content_type)?;

    // Content-Length を信用せずチャンク読みで上限を強制する
    let mut bytes: Vec<u8> = Vec::new();
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| AppError::HttpRequest(format!("failed to read image body: {e}")))?
    {
        if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
            return Err(AppError::Validation(format!(
                "image exceeds size limit ({MAX_IMAGE_BYTES} bytes)"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(FetchedImage {
        url: url_str.to_string(),
        data_uri: build_data_uri(&mime, &bytes),
    })
}

/// 外部画像をユーザーの明示操作（「画像を表示」）でのみ取得する。
/// CSP img-src は緩めず、Rust 側で取得して data URI に変換する。
/// 一部の失敗は全体を失敗させず、取得できた画像だけ返す。
#[tauri::command]
pub async fn fetch_external_images(urls: Vec<String>) -> Result<Vec<FetchedImage>, AppError> {
    let client = build_image_client()?;
    let mut images = Vec::new();
    for url in urls.iter().take(MAX_IMAGES) {
        match fetch_one(&client, url).await {
            Ok(img) => images.push(img),
            Err(e) => {
                eprintln!("[warn] fetch_external_images: skipped {url}: {e}");
            }
        }
    }
    Ok(images)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_image_url ---

    #[test]
    fn test_allows_public_http_and_https() {
        assert!(validate_image_url("https://example.com/a.png").is_ok());
        assert!(validate_image_url("http://example.com/a.png").is_ok());
        assert!(validate_image_url("https://93.184.216.34/a.png").is_ok());
    }

    #[test]
    fn test_rejects_non_http_schemes() {
        for url in [
            "file:///etc/passwd",
            "ftp://example.com/a.png",
            "data:image/png;base64,AAAA",
            "com.haiso666.pigeon://oauth/callback",
        ] {
            assert!(validate_image_url(url).is_err(), "{url} should be rejected");
        }
    }

    #[test]
    fn test_rejects_private_and_loopback_addresses() {
        for url in [
            "http://127.0.0.1/a.png",
            "http://localhost/a.png",
            "http://LOCALHOST/a.png",
            "http://10.0.0.1/a.png",
            "http://192.168.1.1/a.png",
            "http://172.16.0.1/a.png",
            "http://169.254.169.254/latest/meta-data", // リンクローカル（クラウドメタデータ）
            "http://0.0.0.0/a.png",
            "http://[::1]/a.png",
        ] {
            assert!(validate_image_url(url).is_err(), "{url} should be rejected");
        }
    }

    // --- validate_image_content_type ---

    #[test]
    fn test_accepts_common_image_types() {
        assert_eq!(
            validate_image_content_type("image/png").unwrap(),
            "image/png"
        );
        assert_eq!(
            validate_image_content_type("image/svg+xml; charset=utf-8").unwrap(),
            "image/svg+xml"
        );
    }

    #[test]
    fn test_rejects_non_image_types() {
        for ct in ["text/html", "application/octet-stream", "", "image/"] {
            assert!(validate_image_content_type(ct).is_err(), "{ct}");
        }
    }

    #[test]
    fn test_rejects_content_type_with_injection_characters() {
        // data URI の区切り文字（; , 空白等）をサブタイプに含む型は拒否する
        for ct in ["image/png;base64", "image/png,x", "image/a b"] {
            let result = validate_image_content_type(ct);
            if let Ok(mime) = result {
                // パラメータ部として捨てられた場合は許容（"image/png;base64" → "image/png"）
                assert!(
                    mime.strip_prefix("image/").is_some_and(|s| s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"+.-".contains(&b))),
                    "{ct} must not leak separators: {mime}"
                );
            }
        }
    }

    // --- build_data_uri ---

    #[test]
    fn test_builds_base64_data_uri() {
        assert_eq!(
            build_data_uri("image/png", &[0x89, 0x50]),
            "data:image/png;base64,iVA="
        );
    }
}
