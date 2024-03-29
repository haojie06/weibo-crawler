use chrono::{Local, TimeZone, Utc};
use clap::Parser;
use tokio::fs;
use tokio::time::{interval, Duration};
use weibo_crawler::WeiboCrawler;
mod utils;
use std::path::Path;
use utils::*;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, required = true)]
    uid: String,
    #[arg(short, long, default_value = "30")]
    interval_secs: u64,
    #[arg(short, long, default_value = "weibo-data")]
    data_dir: String,
    #[arg(short, default_value = "5", help = "每次获取微博的数量")]
    n: usize,
    #[arg(
        short,
        long,
        help = "删除旧文件",
        action, // 作为flag
    )]
    replace_old_dir: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ctrlc::set_handler(|| {
        println!("exiting...");
        std::process::exit(0);
    })?;
    let args = Args::parse();
    let crawler = WeiboCrawler::new(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string(),
    ).init().await?;

    let mut interval = interval(Duration::from_secs(args.interval_secs));
    let mut latest_created_at = Utc.with_ymd_and_hms(2000, 1, 1, 1, 1, 1).unwrap();
    let data_dir = Path::new(&args.data_dir);
    if args.replace_old_dir && data_dir.exists() {
        println!("deleting old data dir...");
        fs::remove_dir_all(data_dir).await?;
    }
    fs::create_dir_all(data_dir).await?;
    fs::create_dir_all(data_dir.join("medias")).await?;

    loop {
        interval.tick().await;
        let mut new_weibo_count: i32 = 0;
        let mut weibos = match crawler.get_weibos(&args.uid, args.n).await {
            Ok(weibos) => weibos,
            Err(e) => {
                eprintln!("get weibos error: {}", e);
                continue;
            }
        };
        while let Some(weibo) = weibos.pop() {
            if weibo.created_at > latest_created_at {
                println!("{} new weibo:\n{}", Local::now(), weibo);
                new_weibo_count += 1;
                latest_created_at = weibo.created_at.into();
                append_text_to_file(
                    &format!("{}/weibo.txt", &args.data_dir),
                    &format!("{}\n\n", weibo),
                )
                .await?;
                for pic in weibo.pics {
                    if let Err(e) = crawler
                        .download_weibo_file(&pic.url, &format!("{}/medias", &args.data_dir))
                        .await
                    {
                        eprintln!("download file error: {}", e);
                    }
                }
            }
        }
        if new_weibo_count == 0 {
            println!("{} no new weibo found", Local::now());
        }
    }
}
