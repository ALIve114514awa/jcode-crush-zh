use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

pub(crate) async fn read(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<Value> {
    let mut length = None;
    let mut header_bytes = 0;
    loop {
        // Bound each line before allocating it, including malicious headers
        // that never contain a newline.
        let mut line = Vec::new();
        loop {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                bail!("Language server closed stdout");
            }
            let take = available
                .iter()
                .position(|b| *b == b'\n')
                .map_or(available.len(), |i| i + 1);
            if header_bytes + line.len() + take > MAX_HEADER_BYTES {
                bail!("LSP header exceeds 16 KiB");
            }
            let complete = available[take - 1] == b'\n';
            line.extend_from_slice(&available[..take]);
            reader.consume(take);
            if complete {
                break;
            }
        }
        header_bytes += line.len();
        let line = std::str::from_utf8(&line)?.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':')
            && key.eq_ignore_ascii_case("content-length")
        {
            if length.is_some() {
                bail!("Duplicate LSP Content-Length header");
            }
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .context("Invalid LSP Content-Length")?,
            );
        }
    }
    let length = length.context("Missing LSP Content-Length")?;
    if length > MAX_BODY_BYTES {
        bail!("LSP message exceeds 16 MiB");
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body).context("Invalid LSP JSON message")
}

pub(crate) async fn write(writer: &mut (impl AsyncWrite + Unpin), value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn fragmented_messages_use_byte_lengths() {
        let (mut writer, reader) = tokio::io::duplex(7);
        let value = json!({"text":"中文😀"});
        let expected = value.clone();
        let task = tokio::spawn(async move {
            write(&mut writer, &value).await.unwrap();
        });
        assert_eq!(read(&mut BufReader::new(reader)).await.unwrap(), expected);
        task.await.unwrap();
    }

    #[tokio::test]
    async fn invalid_and_oversized_frames_are_rejected() {
        for input in [
            "Content-Length: 16777217\r\n\r\n",
            "Content-Length: -1\r\n\r\n",
            "Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "Other: 2\r\n\r\n{}",
        ] {
            assert!(read(&mut BufReader::new(input.as_bytes())).await.is_err());
        }
        let giant = "a".repeat(MAX_HEADER_BYTES + 1);
        assert!(read(&mut BufReader::new(giant.as_bytes())).await.is_err());
    }
}
