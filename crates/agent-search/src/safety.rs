use crate::types::{PromptInjectionPolicy, SearchError};
use regex::Regex;
use std::net::{IpAddr, Ipv4Addr};
use url::Url;

fn secret_patterns() -> &'static [(&'static str, &'static str)] {
    &[
        (
            r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b",
            "[redacted-email]",
        ),
        (
            r"(?i)\b(?:api[_-]?key|bearer|token|secret)\s*[:=]\s*[^\s]{8,}\b",
            "[redacted-secret]",
        ),
    ]
}

pub fn sanitize_query(input: &str) -> Result<String, SearchError> {
    let mut output = input.trim().to_string();
    for (pattern, replacement) in secret_patterns() {
        let re = Regex::new(pattern).map_err(|err| SearchError::Other(anyhow::anyhow!(err)))?;
        output = re.replace_all(&output, *replacement).into_owned();
    }
    if output.is_empty() {
        return Err(SearchError::Policy(
            "query became empty after sanitization".to_string(),
        ));
    }
    Ok(output)
}

pub fn quarantine_content(input: &str) -> (String, bool) {
    let lowered = input.to_ascii_lowercase();
    let patterns = [
        "ignore previous instructions",
        "system prompt",
        "developer message",
        "tool invocation",
        "exfiltrate",
    ];
    let quarantined = patterns.iter().any(|needle| lowered.contains(needle));
    let text = if quarantined {
        input
            .lines()
            .map(|line| {
                if patterns
                    .iter()
                    .any(|needle| line.to_ascii_lowercase().contains(needle))
                {
                    "[quarantined instruction]"
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        input.to_string()
    };
    (text, quarantined)
}

pub fn block_internal_url(url: &str) -> Result<(), SearchError> {
    let parsed = Url::parse(url)?;
    if matches!(parsed.scheme(), "http" | "https" | "file") {
        if let Some(host) = parsed.host_str() {
            if host.eq_ignore_ascii_case("localhost")
                || host.eq_ignore_ascii_case("localhost.localdomain")
                || host.eq_ignore_ascii_case("0.0.0.0")
            {
                return Err(SearchError::Policy(format!("internal url blocked: {url}")));
            }
            if let Ok(ip) = host.parse::<IpAddr>() {
                if is_private_ip(ip) {
                    return Err(SearchError::Policy(format!("internal url blocked: {url}")));
                }
            }
        }
    }
    Ok(())
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4 == Ipv4Addr::UNSPECIFIED
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unspecified(),
    }
}

pub fn strip_active_html(input: &str) -> String {
    let mut out = input.to_string();
    for pattern in [
        r"(?is)<script\b[^>]*>.*?</script>",
        r"(?is)<style\b[^>]*>.*?</style>",
        r"(?is)<noscript\b[^>]*>.*?</noscript>",
        r"(?is)<!--.*?-->",
    ] {
        if let Ok(re) = Regex::new(pattern) {
            out = re.replace_all(&out, "").into_owned();
        }
    }
    if let Ok(tag_re) = Regex::new(r"(?is)<[^>]+>") {
        out = tag_re.replace_all(&out, " ").into_owned();
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn prompt_injection_policy() -> PromptInjectionPolicy {
    PromptInjectionPolicy::Quarantine
}
