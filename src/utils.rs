use std::error::Error;

use chrono::{DateTime, FixedOffset};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

#[allow(dead_code)]
pub fn parse_weibo_created_at(created_at: &str) -> Result<DateTime<FixedOffset>, Box<dyn Error>> {
    let dt = DateTime::parse_from_str(created_at, "%a %b %d %T %z %Y")?;
    Ok(dt)
}

#[allow(dead_code)]
pub async fn append_text_to_file(path: &str, content: &str) -> Result<(), Box<dyn Error>> {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .await?;
    file.write_all(content.as_bytes()).await?;
    Ok(())
}
