//! The one request husk makes to the CalDAV server itself: creating a
//! list. Everything else goes through vdirsyncer. A list made on the vdir
//! side reaches the server without a name (vdirsyncer's MKCOL carries
//! none, and Radicale then reports the collection path as the name, which
//! the next metasync copies back over the local one), so the server side
//! is created first, with the name, and discover plus metasync bring it
//! down.

use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ureq::http::Request;
use ureq::tls::{RootCerts, TlsConfig};

use crate::config::Caldav;

/// Creates the calendar collection `<url>/<username>/<id>/` holding
/// VTODOs, named `name`. The id doubles as the vdir directory name, so the
/// caller picks it and creates the local side with the same one.
pub fn create_list(caldav: &Caldav, id: &str, name: &str, color: Option<&str>) -> Result<()> {
    let password = password(&caldav.password_command)?;
    let url = format!(
        "{}/{}/{}/",
        caldav.url.trim_end_matches('/'),
        percent_encode(&caldav.username),
        id
    );
    let auth = format!(
        "Basic {}",
        base64(format!("{}:{password}", caldav.username).as_bytes())
    );
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .allow_non_standard_methods(true)
            // The system trust store, so a private CA installed for a
            // self-hosted server works like it does for vdirsyncer.
            .tls_config(
                TlsConfig::builder()
                    .root_certs(RootCerts::PlatformVerifier)
                    .build(),
            )
            .timeout_global(Some(Duration::from_secs(30)))
            .build(),
    );
    let request = Request::builder()
        .method("MKCOL")
        .uri(&url)
        .header("Authorization", auth)
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(mkcol_body(name, color))
        .context("build the MKCOL request")?;
    let response = agent.run(request).with_context(|| format!("MKCOL {url}"))?;
    match response.status().as_u16() {
        201 => Ok(()),
        401 | 403 => bail!("the server refused the credentials for {}", caldav.username),
        405 => bail!("a list with id {id} already exists on the server"),
        status => {
            let reason = response
                .status()
                .canonical_reason()
                .unwrap_or("")
                .to_string();
            // Radicale's error bodies are XML; only a plain-text line helps.
            let text = response.into_body().read_to_string().unwrap_or_default();
            let line = text
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('<'))
                .unwrap_or("");
            bail!("MKCOL {url} answered {status} {reason} {line}");
        }
    }
}

/// An extended MKCOL (RFC 5689): the resource type, the component set and
/// the name in one request, so the collection never exists without a name.
fn mkcol_body(name: &str, color: Option<&str>) -> String {
    let color = color
        .map(|c| format!("<I:calendar-color>{}</I:calendar-color>", escape(c)))
        .unwrap_or_default();
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
            "<D:mkcol xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"",
            " xmlns:I=\"http://apple.com/ns/ical/\"><D:set><D:prop>",
            "<D:resourcetype><D:collection/><C:calendar/></D:resourcetype>",
            "<C:supported-calendar-component-set><C:comp name=\"VTODO\"/>",
            "</C:supported-calendar-component-set>",
            "<D:displayname>{}</D:displayname>{}",
            "</D:prop></D:set></D:mkcol>"
        ),
        escape(name),
        color
    )
}

/// A URL path segment: unreserved characters pass, the rest is encoded.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The first line of the command's output, like vdirsyncer's `password.fetch`.
fn password(command: &[String]) -> Result<String> {
    let (program, args) = command
        .split_first()
        .context("password_command is empty under [caldav] in the config")?;
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("run {program}"))?;
    if !output.status.success() {
        bail!("{program} failed with {}", output.status);
    }
    let text = String::from_utf8(output.stdout).context("password is not UTF-8")?;
    Ok(text.lines().next().unwrap_or("").to_string())
}

/// Standard base64 with padding, for the Basic auth header.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"jacob:s3cret"), "amFjb2I6czNjcmV0");
    }

    #[test]
    fn usernames_are_safe_in_the_path() {
        assert_eq!(percent_encode("jacob"), "jacob");
        assert_eq!(percent_encode("j c@x.org#1"), "j%20c%40x.org%231");
    }

    #[test]
    fn mkcol_body_names_a_vtodo_calendar_and_escapes_the_name() {
        let body = mkcol_body("Tom & Jerry <3", Some("#FFCC00"));
        assert!(body.contains("<C:comp name=\"VTODO\"/>"));
        assert!(body.contains("<D:displayname>Tom &amp; Jerry &lt;3</D:displayname>"));
        assert!(body.contains("<I:calendar-color>#FFCC00</I:calendar-color>"));
        assert!(!mkcol_body("x", None).contains("calendar-color"));
    }
}
