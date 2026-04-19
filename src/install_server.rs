//! HTTP install server hosted by the host process.
//!
//! Three endpoints, all public on the host's HTTP port:
//!
//! - `GET /install.sh` — session-specific bash installer. Baked with this
//!   session's share code, version, HTTP base, and hex token.
//! - `GET /install.ps1` — session-specific PowerShell installer. Same.
//! - `GET /binary?platform=…` — the host process's current executable
//!   bytes. Response carries `X-Binary-HMAC: <hex>` = HMAC-SHA256 of the
//!   bytes using the session token as key. Scripts verify this before
//!   executing the downloaded binary.
//!
//! Security posture (v1):
//! - HTTP only (no TLS). The HMAC protects against passive MITM binary
//!   substitution, because the attacker would need the session token
//!   (delivered out-of-band in the share code) to forge a matching HMAC.
//! - The host serves its own-platform binary unconditionally — cross-OS
//!   friends have to install manually for now.
//! - No rate-limiting. A malicious peer who has the share code could
//!   download the binary repeatedly; bandwidth abuse is bounded by
//!   "your friends."

use anyhow::{Context, Result, anyhow};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io::Read;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::thread::JoinHandle;
use tiny_http::{Header, Method, Request, Response, Server};

type HmacSha256 = Hmac<Sha256>;

pub struct InstallServer {
    _handle: JoinHandle<()>,
    pub bash_one_liner: String,
    pub pwsh_one_liner: String,
}

pub fn start(port: u16, session_token: [u8; 16], share_code: &crate::share::ShareCode) -> Result<InstallServer> {
    let addr = format!("0.0.0.0:{port}");
    let server = Server::http(&addr)
        .map_err(|e| anyhow!("start HTTP install server on {addr}: {e}"))?;

    let version = env!("CARGO_PKG_VERSION").to_string();
    let token_hex = to_hex(&session_token);

    // Advertise the STUN-discovered public IP in the printed one-liners —
    // they're what the host pastes into Discord. The per-request handler
    // below will rewrite both $httpBase and the embedded share code to
    // match whatever Host header the client used, so local testing via
    // 127.0.0.1 works even when STUN found a public IP.
    let adv_base = format!("http://{}:{}", share_code.ip, share_code.http_port);
    let bash_one_liner = format!("curl -sSfL {adv_base}/install.sh | sh");
    let pwsh_one_liner = format!("iwr {adv_base}/install.ps1 -UseBasicParsing | iex");

    // Load the current binary once and cache it.
    let exe_path = std::env::current_exe().context("locate current_exe")?;
    let mut binary = Vec::new();
    std::fs::File::open(&exe_path)
        .with_context(|| format!("open {exe_path:?}"))?
        .read_to_end(&mut binary)?;
    let binary = Arc::new(binary);

    // Precompute HMAC — same for every download of this session.
    let mut mac = HmacSha256::new_from_slice(&session_token)
        .context("hmac init")?;
    mac.update(&binary);
    let hmac_hex = Arc::new(to_hex(&mac.finalize().into_bytes()));

    let default_code = share_code.clone();
    let version_arc = Arc::new(version);
    let token_hex_arc = Arc::new(token_hex);
    let binary_arc = binary;

    let handle = std::thread::Builder::new()
        .name("install-server".into())
        .spawn(move || {
            tracing::info!(port, "install HTTP server running");
            for request in server.incoming_requests() {
                if !matches!(request.method(), Method::Get) {
                    let _ = request
                        .respond(Response::from_string("method not allowed").with_status_code(405));
                    continue;
                }
                let url = request.url().to_string();
                let path = url.split('?').next().unwrap_or("").to_string();
                let result = match path.as_str() {
                    "/install.sh" => {
                        let (base, code) = effective_base_and_code(&request, &default_code);
                        let body = build_bash_script(&code, &version_arc, &base, &token_hex_arc);
                        respond_text(request, &body, "text/plain")
                    }
                    "/install.ps1" => {
                        let (base, code) = effective_base_and_code(&request, &default_code);
                        let body = build_pwsh_script(&code, &version_arc, &base, &token_hex_arc);
                        respond_text(request, &body, "text/plain")
                    }
                    "/binary" => respond_binary(request, &binary_arc, &hmac_hex),
                    _ => {
                        let _ = request
                            .respond(Response::from_string("not found").with_status_code(404));
                        Ok(())
                    }
                };
                if let Err(e) = result {
                    tracing::warn!(%e, "install server response error");
                }
            }
        })
        .context("spawn install server thread")?;

    Ok(InstallServer {
        _handle: handle,
        bash_one_liner,
        pwsh_one_liner,
    })
}

/// Read the request's `Host` header. Returns the raw value as received
/// (e.g. `"127.0.0.1:4647"` or `"203.0.113.7:4647"`).
fn request_host(req: &Request) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("Host"))
        .map(|h| h.value.as_str().to_string())
}

/// Pick the URL base + share code to bake into a script for this request.
/// When the request's Host header resolves to an IPv4 address, we rewrite
/// the share code's IP to match so the script's `connect $code` step
/// targets the same host the script was fetched from (critical for local
/// testing where the public IP would hairpin-fail).
fn effective_base_and_code(
    req: &Request,
    default_code: &crate::share::ShareCode,
) -> (String, String) {
    let host = request_host(req).unwrap_or_else(|| {
        format!("{}:{}", default_code.ip, default_code.http_port)
    });
    let base = format!("http://{host}");
    // Attempt to parse host as "ipv4[:port]" — if it succeeds, rebuild the
    // share code with that IP so the game connection matches the install
    // fetch. Otherwise (DNS name, unparseable), fall through to default.
    let ip = parse_host_ipv4(&host);
    let code = match ip {
        Some(ip) => {
            let mut c = default_code.clone();
            c.ip = ip;
            c.encode()
        }
        None => default_code.encode(),
    };
    (base, code)
}

fn parse_host_ipv4(host: &str) -> Option<Ipv4Addr> {
    let head = match host.rsplit_once(':') {
        Some((h, _port)) => h,
        None => host,
    };
    head.parse::<Ipv4Addr>().ok()
}

fn respond_text(req: Request, body: &str, content_type: &str) -> Result<()> {
    let mut resp = Response::from_string(body.to_string());
    let header = Header::from_bytes(b"Content-Type".as_ref(), content_type.as_bytes())
        .map_err(|_| anyhow!("build Content-Type header"))?;
    resp.add_header(header);
    req.respond(resp).map_err(|e| anyhow!("respond: {e}"))?;
    Ok(())
}

fn respond_binary(
    req: Request,
    binary: &Arc<Vec<u8>>,
    hmac_hex: &Arc<String>,
) -> Result<()> {
    let bytes = binary.clone();
    let len = bytes.len();
    let cursor = std::io::Cursor::new((*bytes).clone());
    let mut resp = Response::new(
        tiny_http::StatusCode(200),
        vec![],
        cursor,
        Some(len),
        None,
    );
    let ct = Header::from_bytes(
        b"Content-Type".as_ref(),
        b"application/octet-stream".as_ref(),
    )
    .map_err(|_| anyhow!("build Content-Type"))?;
    resp.add_header(ct);
    let h = Header::from_bytes(
        b"X-Binary-HMAC".as_ref(),
        hmac_hex.as_bytes(),
    )
    .map_err(|_| anyhow!("build HMAC header"))?;
    resp.add_header(h);
    req.respond(resp).map_err(|e| anyhow!("respond: {e}"))?;
    Ok(())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn build_bash_script(code: &str, version: &str, http_base: &str, token_hex: &str) -> String {
    format!(
        r#"#!/bin/sh
# terminal_hell auto-install + connect. Served by an active host session;
# the HMAC check below verifies the binary actually came from the host
# whose share code you're about to connect to.

set -eu
CODE="{code}"
VERSION="{version}"
HTTP_BASE="{http_base}"
TOKEN_HEX="{token_hex}"

# Already installed at the right version? Skip straight to connect.
if command -v terminal_hell >/dev/null 2>&1; then
    CUR=$(terminal_hell --version 2>/dev/null | awk '{{print $NF}}')
    if [ "$CUR" = "$VERSION" ]; then
        exec terminal_hell connect "$CODE"
    fi
fi

OS=$(uname -s)
ARCH=$(uname -m)
case "$OS" in
    Linux) PLATFORM="linux-$ARCH" ;;
    Darwin) PLATFORM="macos-$ARCH" ;;
    *) echo "Unsupported OS $OS. Install manually from the project README." >&2; exit 1 ;;
esac

if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required." >&2
    exit 1
fi
if ! command -v openssl >/dev/null 2>&1; then
    echo "openssl is required for binary verification." >&2
    exit 1
fi

TMPFILE=$(mktemp)
HEADERS=$(mktemp)
trap 'rm -f "$TMPFILE" "$HEADERS"' EXIT

curl -sSfL -D "$HEADERS" -o "$TMPFILE" "$HTTP_BASE/binary?platform=$PLATFORM"
HMAC_HEADER=$(awk 'tolower($1) == "x-binary-hmac:" {{print $2}}' "$HEADERS" | tr -d '\r\n')

if [ -z "$HMAC_HEADER" ]; then
    echo "Install server didn't return an HMAC. Aborting." >&2
    exit 1
fi

COMPUTED=$(openssl dgst -sha256 -mac HMAC -macopt "hexkey:$TOKEN_HEX" -hex "$TMPFILE" | awk '{{print $NF}}')

if [ "$HMAC_HEADER" != "$COMPUTED" ]; then
    echo "Binary HMAC mismatch. Aborting. Your host's share code may not match this install server." >&2
    exit 1
fi

mkdir -p "$HOME/.local/bin"
install -m 0755 "$TMPFILE" "$HOME/.local/bin/terminal_hell"
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) export PATH="$HOME/.local/bin:$PATH" ;;
esac

exec terminal_hell connect "$CODE"
"#
    )
}

fn build_pwsh_script(code: &str, version: &str, http_base: &str, token_hex: &str) -> String {
    format!(
        r#"# terminal_hell auto-install + connect (PowerShell).
$ErrorActionPreference = 'Stop'
$code = "{code}"
$version = "{version}"
$httpBase = "{http_base}"
$tokenHex = "{token_hex}"

$existing = Get-Command terminal_hell -ErrorAction SilentlyContinue
if ($existing) {{
    $cur = (& terminal_hell --version) -split ' ' | Select-Object -Last 1
    if ($cur.Trim() -eq $version) {{
        & terminal_hell connect $code
        exit $LASTEXITCODE
    }}
}}

$arch = $env:PROCESSOR_ARCHITECTURE
switch ($arch) {{
    "AMD64" {{ $platform = "windows-x86_64" }}
    "ARM64" {{ $platform = "windows-aarch64" }}
    default {{ throw "Unsupported arch: $arch. Install manually from the project README." }}
}}

$tmp = [IO.Path]::GetTempFileName()
try {{
    $resp = Invoke-WebRequest -UseBasicParsing `
        -Uri "$httpBase/binary?platform=$platform" `
        -OutFile $tmp -PassThru

    $hmacHeader = $null
    foreach ($k in $resp.Headers.Keys) {{
        if ($k.ToLower() -eq 'x-binary-hmac') {{
            $hmacHeader = $resp.Headers[$k]
            break
        }}
    }}
    if (-not $hmacHeader) {{ throw "Install server didn't return an HMAC." }}

    $keyBytes = New-Object byte[] ($tokenHex.Length / 2)
    for ($i = 0; $i -lt $tokenHex.Length; $i += 2) {{
        $keyBytes[$i / 2] = [Convert]::ToByte($tokenHex.Substring($i, 2), 16)
    }}
    $hmac = New-Object System.Security.Cryptography.HMACSHA256
    $hmac.Key = $keyBytes
    $fileBytes = [IO.File]::ReadAllBytes($tmp)
    $computed = ([BitConverter]::ToString($hmac.ComputeHash($fileBytes))).Replace("-","").ToLower()
    if ($computed -ne $hmacHeader.ToString().ToLower()) {{
        throw "Binary HMAC mismatch. Aborting."
    }}

    $installDir = Join-Path $env:USERPROFILE "bin"
    [void](New-Item -Force -ItemType Directory -Path $installDir)
    $dest = Join-Path $installDir "terminal_hell.exe"
    Move-Item -Force $tmp $dest
    $env:PATH = "$installDir;$env:PATH"
    & $dest connect $code
    exit $LASTEXITCODE
}} finally {{
    if (Test-Path $tmp) {{ Remove-Item -Force $tmp -ErrorAction SilentlyContinue }}
}}
"#
    )
}
