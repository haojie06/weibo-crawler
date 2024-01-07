use chrono::{DateTime, FixedOffset, Local, TimeZone, Utc};
use regex::Regex;
use reqwest::{
    self,
    header::{COOKIE, REFERER},
    redirect::Policy,
    Client, StatusCode,
};
use serde_json;
use std::{
    env,
    error::Error,
    fmt::{self, Display},
    fs::{self, OpenOptions},
    io::Write,
};
use tokio::time::{interval, Duration};

#[derive(Debug)]
enum CustomError {
    GenVistorError,
    ParseVisitorError(String),
    GetWeiboError(String),
    DownloadFileError,
}

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CustomError::GenVistorError => write!(f, "gen vistor error"),
            CustomError::ParseVisitorError(ref msg) => write!(f, "parse vistor error: {}", msg), // TODO 学习 ref 的使用
            CustomError::GetWeiboError(ref msg) => write!(f, "get weibo error: {}", msg),
            CustomError::DownloadFileError => write!(f, "download file error"),
        }
    }
}

impl Error for CustomError {}

#[derive(Debug)]
struct WeiboPic {
    pic_id: String,
    pic_type: String,
    url: String,
    video_url: String,
}

impl Display for WeiboPic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = String::new();
        s.push_str(&format!(
            "id: {} type: {} url: {}",
            self.pic_id, self.pic_type, self.url
        ));
        if self.video_url != "" {
            s.push_str(&format!(" video_url: {}", self.video_url));
        }
        write!(f, "{}", s)
    }
}
#[derive(Debug)]
struct Weibo {
    text_raw: String,
    created_at: DateTime<FixedOffset>,
    pics: Vec<WeiboPic>,
}

impl Display for Weibo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let pic_str = self
            .pics
            .iter()
            .map(|p| format!("{}", p))
            .collect::<Vec<String>>()
            .join("\n");
        write!(
            f,
            "created_at: {}\ntext: {}\n{} pics: \n{}",
            self.created_at,
            self.text_raw,
            self.pics.len(),
            pic_str
        )
    }
}

// 生成 vistor cookies
async fn gen_vistor(client: &Client) -> Result<String, Box<dyn std::error::Error>> {
    let params = [
        ("cb", "visitor_gray_callback"),
        ("tid", ""),
        ("from", "weibo"),
    ];
    let resp = client
        .post("https://passport.weibo.com/visitor/genvisitor2")
        .form(&params)
        .send()
        .await?;
    let re = Regex::new(r"visitor_gray_callback\((.*?)\);").unwrap();
    let cookies_str = match resp.status() {
        StatusCode::OK => match resp.text().await {
            Ok(body) => {
                let json_str = re
                    .captures(&body)
                    .ok_or(CustomError::ParseVisitorError(body.clone()))?;
                let deserialized: serde_json::Value =
                    serde_json::from_str(&json_str[1]).map_err(|e| {
                        eprintln!("parse response error: {}", e);
                        CustomError::ParseVisitorError(body.clone())
                    })?;

                // println!("{:#?}", deserialized);
                let mut cookies_string = String::new();
                let sub = deserialized["data"]["sub"]
                    .as_str()
                    .ok_or_else(|| {
                        eprintln!("failed to get sub from response {:#?}", deserialized);
                        CustomError::ParseVisitorError("failed to get sub".to_string())
                    })?
                    .to_string();
                cookies_string.push_str(&format!("SUB={}; ", sub));
                let subp = deserialized["data"]["subp"]
                    .as_str()
                    .ok_or_else(|| {
                        eprintln!("failed to get subp from response {:#?}", deserialized);
                        CustomError::ParseVisitorError("failed to get subp".to_string())
                    })?
                    .to_string();
                cookies_string.push_str(&format!("SUBP={}; ", subp));
                Ok(cookies_string)
            }
            Err(e) => {
                eprintln!("parse response error: {}", e);
                Err(CustomError::ParseVisitorError(e.to_string()))
            }
        },
        s => {
            eprintln!("request failed: {}", s);
            Err(CustomError::GenVistorError)
        }
    }?;
    Ok(cookies_str)
}

fn parse_weibo_created_at(created_at: &str) -> Result<DateTime<FixedOffset>, Box<dyn Error>> {
    let dt = DateTime::parse_from_str(created_at, "%a %b %d %T %z %Y")?;
    Ok(dt)
}
// 获取微博，指定条数
async fn get_weibo(
    client: &Client,
    uid: &str,
    cookies_str: &str,
    n: usize,
) -> Result<Vec<Weibo>, Box<dyn std::error::Error>> {
    let url = format!(
        "https://weibo.com/ajax/statuses/mymblog?uid={}&page=1&feature=0",
        uid
    );
    let resp = client.get(url).header(COOKIE, cookies_str).send().await?;
    match resp.status() {
        StatusCode::OK => match resp.text().await {
            Ok(body) => {
                // println!("response body: {}", body);
                let deserialized: serde_json::Value = serde_json::from_str(&body)?;
                let t = deserialized["data"]["list"]
                    .as_array()
                    .expect("failed to get list");
                let mut weibos: Vec<Weibo> = vec![];
                for wb in t.iter().take(n) {
                    // println!("{:#?}", wb);
                    let text_raw = wb["text_raw"].as_str().expect("failed to get text_raw");
                    let created_at = wb["created_at"]
                        .as_str()
                        .map(|t| parse_weibo_created_at(t))
                        .expect("failed to get created_at")?;

                    let pic_ids = wb["pic_ids"]
                        .as_array()
                        .expect("failed to get pic_ids")
                        .iter()
                        .map(|id| id.as_str().expect("failed to get pic_id"))
                        .collect::<Vec<&str>>(); // XXX 注意这里的标注
                    let pic_infos = wb["pic_infos"].as_object(); // 图片可能为空

                    let mut weibo = Weibo {
                        text_raw: text_raw.to_string(),
                        created_at,
                        pics: vec![],
                    };
                    if let Some(pic_infos) = pic_infos {
                        for id in pic_ids {
                            let pic_info =
                                pic_infos[id].as_object().expect("failed to get pic_info");
                            let largest = pic_info["largest"]
                                .as_object()
                                .expect("failed to get largest");
                            let url = largest["url"].as_str().expect("failed to get largest url");
                            let pic_type =
                                pic_info["type"].as_str().expect("failed to get pic_type");
                            let video_url = pic_info
                                .get("video")
                                .map_or("", |v| v.as_str().unwrap_or(""));
                            let weibo_pic = WeiboPic {
                                pic_id: id.to_string(),
                                pic_type: pic_type.to_string(),
                                url: url.to_string(),
                                video_url: video_url.to_string(),
                            };
                            weibo.pics.push(weibo_pic);
                        }
                    }

                    weibos.push(weibo);
                }
                Ok(weibos)
            }
            Err(e) => {
                eprintln!("response error: {}", e);
                Err(CustomError::GetWeiboError(e.to_string()))?
            }
        },
        s => {
            eprintln!("request failed: {}", s);
            Err(CustomError::GetWeiboError(s.to_string()))?
        }
    }
}

fn append_to_file(path: &str, content: &str) -> Result<(), Box<dyn Error>> {
    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

async fn download_weibo_file(client: &Client, url: &str, path: &str) -> Result<(), Box<dyn Error>> {
    println!("downloading {} to {}", url, path);
    let resp = client
        .get(url)
        .header(REFERER, "https://weibo.com/")
        .send()
        .await?;
    println!("response status: {}", resp.status());
    match resp.status() {
        StatusCode::OK => {
            let mut file = OpenOptions::new().write(true).create(true).open(path)?;
            let bytes = resp.bytes().await?;
            file.write_all(&bytes)?;
            println!("downloaded {} bytes", bytes.len());
            Ok(())
        }
        s => {
            eprintln!("request failed: {}", s);
            Err(CustomError::DownloadFileError)?
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ctrlc::set_handler(|| {
        println!("exiting...");
        std::process::exit(0);
    })?;
    fs::create_dir_all("weibo/medias")?;
    let args: Vec<String> = env::args().collect();
    let uid = args.get(1).expect("uid is required").to_owned();
    let client = reqwest::Client::builder()
        .redirect(Policy::none())
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;
    let mut interval = interval(Duration::from_secs(30));
    let mut latest_created_at = Utc.with_ymd_and_hms(2000, 1, 1, 1, 1, 1).unwrap();
    loop {
        interval.tick().await;
        let cookies_str = gen_vistor(&client).await?;
        let mut weibos = get_weibo(&client, &uid, &cookies_str, 5).await?;
        let mut new_weibo_count = 0;
        while let Some(weibo) = weibos.pop() {
            if weibo.created_at > latest_created_at {
                println!("{} new weibo:\n{}", Local::now(), weibo);
                new_weibo_count += 1;
                latest_created_at = weibo.created_at.into();
                append_to_file("weibo/weibo.txt", &format!("{}\n\n", weibo))?;
                for pic in weibo.pics {
                    let file_name = pic
                        .url
                        .split("/")
                        .last()
                        .unwrap()
                        .split("?")
                        .next()
                        .unwrap();
                    let path = format!("weibo/medias/{}", file_name);
                    if let Err(e) = download_weibo_file(&client, &pic.url, &path).await {
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
