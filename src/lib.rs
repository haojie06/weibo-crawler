use regex::Regex;
use reqwest::{
    header::{COOKIE, REFERER},
    Client, StatusCode,
};
use std::{error::Error, fmt};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
mod models;
use models::*;
mod utils;
use utils::*;

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
            CustomError::ParseVisitorError(ref msg) => write!(f, "parse vistor error: {}", msg),
            CustomError::GetWeiboError(ref msg) => write!(f, "get weibo error: {}", msg),
            CustomError::DownloadFileError => write!(f, "download file error"),
        }
    }
}

impl Error for CustomError {}
pub struct WeiboCrawler {
    client: Client,
    cookies_string: String,
}

impl WeiboCrawler {
    pub fn new(user_agent: String) -> WeiboCrawler {
        let client = Client::builder()
            .user_agent(user_agent)
            .build()
            .expect("build client failed");
        WeiboCrawler {
            client,
            cookies_string: "".to_string(),
        }
    }

    pub async fn init(mut self) -> Result<Self, Box<dyn std::error::Error>> {
        let cookies_string = self.gen_vistor_cookies_string().await?;
        self.cookies_string = cookies_string;
        Ok(self)
    }
    // 生成访客模式cookies
    async fn gen_vistor_cookies_string(&self) -> Result<String, Box<dyn std::error::Error>> {
        let params = [
            ("cb", "visitor_gray_callback"),
            ("tid", ""),
            ("from", "weibo"),
        ];
        let resp = self
            .client
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
                    let deserialized: serde_json::Value = serde_json::from_str(&json_str[1])
                        .map_err(|e| {
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

    pub async fn get_weibos(
        &self,
        uid: &str,
        n: usize,
    ) -> Result<Vec<Weibo>, Box<dyn std::error::Error>> {
        let url = format!(
            "https://weibo.com/ajax/statuses/mymblog?uid={}&page=1&feature=0",
            uid
        );
        let resp = self
            .client
            .get(url)
            .header(COOKIE, &self.cookies_string)
            .send()
            .await?;
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
                                let url =
                                    largest["url"].as_str().expect("failed to get largest url");
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

    pub async fn download_weibo_file(
        &self,
        url: &str,
        data_dir: &str,
    ) -> Result<(), Box<dyn Error>> {
        let file_name = url.split("/").last().unwrap().split("?").next().unwrap();
        println!("downloading {} {} to {}", file_name, url, data_dir);
        let file_path: String = format!("{}/{}", data_dir, file_name);
        let resp = self
            .client
            .get(url)
            .header(REFERER, "https://weibo.com/")
            .send()
            .await?;
        match resp.status() {
            StatusCode::OK => {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(file_path)
                    .await?;
                let bytes = resp.bytes().await?;
                file.write_all(&bytes).await?;
                // println!("downloaded {} bytes", bytes.len());
                Ok(())
            }
            s => {
                eprintln!("request failed: {}", s);
                Err(CustomError::DownloadFileError)?
            }
        }
    }
}
