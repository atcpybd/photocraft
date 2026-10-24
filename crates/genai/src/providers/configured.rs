//! Shared trust boundary for user configured image services.

use url::{Host, Url};

use crate::{API_KEY, Credentials, GenError, HttpRequest, HttpResponse, HttpTransport, MAX_RESPONSE, Result};

/// A configured endpoint has no embedded secrets. Plain HTTP is restricted to loopback.
pub(crate) fn endpoint(raw: &str) -> Result<String> {
    let url = checked_url(raw, false)?;
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

pub(crate) fn checked_url(raw: &str, allow_query: bool) -> Result<Url> {
    let invalid = || GenError::Invalid("use an HTTPS endpoint, or HTTP on localhost, without URL credentials, query or fragment".into());
    if raw.len() > 4096 || raw.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\') {
        return Err(invalid());
    }
    let url = Url::parse(raw).map_err(|_| invalid())?;
    let authority = raw.split("://").nth(1).and_then(|s| s.split('/').next()).unwrap_or("");
    if authority.contains('@') || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() || (!allow_query && url.query().is_some())
    {
        return Err(invalid());
    }
    let loopback = match url.host() {
        Some(Host::Domain("localhost")) => true,
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if url.host().is_none() || !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return Err(invalid());
    }
    Ok(url)
}

pub(crate) fn config(raw: &str) -> Result<serde_json::Value> {
    if raw.len() > 2 * 1024 * 1024 {
        return Err(GenError::Invalid("the provider configuration is too large".into()));
    }
    serde_json::from_str(raw).map_err(|_| GenError::Invalid("enter a JSON provider configuration".into()))
}

pub(crate) fn string<'a>(value: &'a serde_json::Value, name: &str) -> Result<&'a str> {
    value
        .get(name)
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| GenError::Invalid(format!("the provider configuration needs `{name}`")))
}

pub(crate) fn validate_credentials(creds: &Credentials) -> Result<()> {
    if creds.get(API_KEY).is_some_and(|k| k.expose().chars().any(char::is_control)) {
        return Err(GenError::Invalid("the API key contains a control character".into()));
    }
    Ok(())
}

pub(crate) fn authorize(request: HttpRequest, creds: &Credentials) -> HttpRequest {
    match creds.get(API_KEY) {
        Some(key) => request.header("authorization", format!("Bearer {}", key.expose())),
        None => request,
    }
}

/// A transport can echo headers in its own error, so scrub it at the provider boundary.
pub(crate) fn scrub_error(err: GenError, creds: &Credentials) -> GenError {
    match err {
        GenError::Network(s) => GenError::Network(creds.scrub(&s)),
        GenError::Http { status, message } => GenError::Http { status, message: creds.scrub(&message) },
        GenError::Invalid(s) => GenError::Invalid(creds.scrub(&s)),
        GenError::Refused(s) => GenError::Refused(creds.scrub(&s)),
        GenError::Moderated(s) => GenError::Moderated(creds.scrub(&s)),
        other => other,
    }
}

pub(crate) fn send(http: &dyn HttpTransport, request: HttpRequest, creds: &Credentials) -> Result<HttpResponse> {
    let response = http.send(request).map_err(|err| scrub_error(err, creds))?;
    if response.body.len() > MAX_RESPONSE {
        return Err(GenError::BadResponse("the response is too large"));
    }
    if !response.is_success() {
        return Err(super::common::http_error(&response, creds));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_validation_uses_parsed_hosts() {
        for raw in ["http://localhost:8188", "http://127.0.0.2:8188/", "http://[::1]:8188", "https://remote.example/v1"] {
            assert!(endpoint(raw).is_ok(), "{raw}");
        }
        for raw in [
            "http://localhost.evil:8188",
            "http://192.168.1.1:8188",
            "http://[::ffff:192.168.1.1]",
            "http://localhost@evil",
            "https://@evil",
            "https://host?",
            "https://host#",
            "https://host:bad",
        ] {
            assert!(endpoint(raw).is_err(), "{raw}");
        }
    }
}
