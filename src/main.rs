use anyhow::{bail, Context, Result};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use clap::Parser;
use colored::Colorize;
use futures::StreamExt;
use std::path::PathBuf;
use url::Url;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    url: Url,
    /// Provide the request body here
    #[arg(long)]
    data: Option<String>,
    /// Provide the request body in $EDITOR
    #[arg(long)]
    editor: bool,
    /// TLS client certificate auth; specify the certificate file
    #[arg(long)]
    cert: Option<PathBuf>,
    /// TLS client certificate auth; specify the key file;
    /// if `--cert` is provided but `--key` is not, the keyfile
    /// location will be guessed from the certfile location.
    #[arg(long)]
    key: Option<PathBuf>,
    /// URL/endpoint is gRPC server streaming
    #[arg(long = "stream")]
    server_streaming: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Cli::parse();
    let mut client_builder = reqwest::Client::builder().http2_prior_knowledge();

    // TLS client certificate auth
    if let Some(cert) = args.cert {
        let cert_pem = std::fs::read(&cert)
            .with_context(|| format!("failed to read cert file: {}", cert.display()))?;
        let key_pem = if let Some(key) = args.key {
            std::fs::read(&key)
                .with_context(|| format!("failed to read key file: {}", key.display()))?
        } else {
            let mut key_path = cert.clone();
            key_path.set_extension("key");
            std::fs::read(&key_path)
                .with_context(|| format!("failed to read key file: {}", key_path.display()))?
        };
        client_builder =
            client_builder.identity(reqwest::Identity::from_pkcs8_pem(&cert_pem, &key_pem)?);
    }

    let client = client_builder.build()?;
    let body = if let Some(data) = args.data {
        Some(data)
    } else if args.editor {
        let content = edit::edit("")?;
        Some(content)
    } else {
        None
    };
    let payload = if let Some(body) = body {
        let mut bytes = BytesMut::new();
        bytes.put_u8(0); // no compression
        bytes.put_u32(body.len() as u32); // payload length
        bytes.put_slice(body.as_bytes()); // payload
        bytes.freeze()
    } else {
        Bytes::from_static(&[0, 0, 0, 0, 0])
    };
    let res = client
        .post(args.url)
        .header("Content-Type", "application/grpc+json")
        .body(payload)
        .send()
        .await?;
    if !res.status().is_success() {
        let status = res.status().as_u16();
        let reason = res.status().canonical_reason().unwrap_or("");
        eprintln!("{}", format!("HTTP {} {}", status, reason).red().bold());
        let res_body = res.text().await?;
        println!("{}", res_body);
        std::process::exit(1);
    } else {
        let status = res.status().as_u16();
        let reason = res.status().canonical_reason().unwrap_or("");
        eprintln!(
            "{}",
            format!("HTTP {} {}", status, reason).bright_white().bold()
        );
        if let Some(code) = res.headers().get("grpc-status") {
            let code = code.to_str()?.parse::<u8>()?;
            let code_reason = match code {
                0 => "OK",
                1 => "CANCELLED",
                2 => "UNKNOWN",
                3 => "INVALID_ARGUMENT",
                4 => "DEADLINE_EXCEEDED",
                5 => "NOT_FOUND",
                6 => "ALREADY_EXISTS",
                7 => "PERMISSION_DENIED",
                8 => "RESOURCE_EXHAUSTED",
                9 => "FAILED_PRECONDITION",
                10 => "ABORTED",
                11 => "OUT_OF_RANGE",
                12 => "UNIMPLEMENTED",
                13 => "INTERNAL",
                14 => "UNAVAILABLE",
                15 => "DATA_LOSS",
                _ => "???",
            };
            if code == 0 {
                eprintln!(
                    "{}",
                    format!("GRPC {} {}", code, code_reason)
                        .bright_white()
                        .bold()
                );
            } else {
                eprintln!("{}", format!("GRPC {} {}", code, code_reason).red().bold());
            }
        }
        if args.server_streaming {
            handle_server_streaming_response(res).await?;
        } else {
            handle_unary_response(res).await?;
        }
    }
    Ok(())
}

async fn handle_unary_response(res: reqwest::Response) -> Result<()> {
    let mut buf = res.bytes().await?;
    if buf.len() < 5 {
        bail!("invalid Length-Prefixed-Message, expected at least 5 bytes");
    }
    let _compressed = buf.get_u8();
    let len = buf.get_u32();
    if len == 0 {
        eprintln!("WARNING: empty response");
        return Ok(());
    }
    let body = buf.get(..).unwrap();
    let text = std::str::from_utf8(body)?;
    println!("{}", text);
    Ok(())
}

async fn handle_server_streaming_response(res: reqwest::Response) -> Result<()> {
    let mut stream = res.bytes_stream();
    while let Some(res) = stream.next().await {
        let mut frame = res?;
        if frame.len() < 5 {
            bail!("invalid Length-Prefixed-Message, expected at least 5 bytes");
        }
        let _compressed = frame.get_u8();
        let len = frame.get_u32();
        if len == 0 {
            eprintln!("WARNING: empty response");
            return Ok(());
        }
        let body = frame.get(..).unwrap();
        let text = std::str::from_utf8(body)?;
        println!("{}", text);
    }
    Ok(())
}
