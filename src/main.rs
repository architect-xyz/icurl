use anyhow::Result;
use bytes::{BufMut, Bytes, BytesMut};
use clap::Parser;
use colored::Colorize;
use url::Url;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    url: Url,
    /// Provide the request body in $EDITOR
    #[arg(long)]
    editor: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Cli::parse();
    let client = reqwest::Client::builder().http2_prior_knowledge().build()?;
    let body = if args.editor {
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
        let res_body = res.text().await?;
        println!("{}", res_body);
    }
    Ok(())
}
