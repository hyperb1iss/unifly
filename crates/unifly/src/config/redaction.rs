//! Conservative masking for URLs, including malformed userinfo.

/// Mask userinfo even when unescaped delimiters make the URL invalid.
/// Ambiguous path text is hidden too: a numeric password prefix can look like
/// a port, and an `@` inside a password can look like the userinfo terminator.
pub fn redact_url_userinfo(url: &str) -> String {
    let scheme_end = url.find("://").filter(|&end| {
        let scheme = &url[..end];
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c))
    });
    let start = match scheme_end {
        Some(index) => index + 3,
        None if url.starts_with("//") => 2,
        None => 0,
    };
    let rest = &url[start..];
    let authority = rest
        .trim_start_matches(['/', '\\'])
        .split(['/', '\\', '?', '#'])
        .next()
        .unwrap_or_default();
    if let Some(at) = rest.rfind('@')
        && (authority.contains('@') || rest[..at].contains(':'))
    {
        format!("{}****{}", &url[..start], &rest[at..])
    } else {
        url.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::redact_url_userinfo;

    #[test]
    fn malformed_passwords_never_survive_masking() {
        for secret in [
            "123/secret",
            "p@ss/word",
            "123?secret",
            "123#secret",
            "a@b/c@d/e",
            "p://ss/word",
        ] {
            for prefix in ["https://", "//", ""] {
                let url = format!("{prefix}admin:{secret}@host:99999");
                assert_eq!(
                    redact_url_userinfo(&url),
                    format!("{prefix}****@host:99999")
                );
            }
        }
    }

    #[test]
    fn extra_slashes_and_delimiters_cannot_hide_credentials() {
        for (url, expected) in [
            ("https:///token@host", "https://****@host"),
            ("////token@host", "//****@host"),
            ("https://\\token@host", "https://****@host"),
            ("\\\\token@host", "****@host"),
            ("https:///token@host:99999", "https://****@host:99999"),
            ("https:///admin:secret@host", "https://****@host"),
            (
                "https:////admin:secret@host:99999",
                "https://****@host:99999",
            ),
            ("///admin:secret@host:99999", "//****@host:99999"),
            ("https://?admin:secret@host", "https://****@host"),
            ("https://#admin:secret@host", "https://****@host"),
        ] {
            assert_eq!(redact_url_userinfo(url), expected);
        }
    }
}
