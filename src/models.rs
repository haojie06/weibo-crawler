use chrono::{DateTime, FixedOffset};
use std::fmt::{self, Display};

#[derive(Debug)]
pub struct WeiboPic {
    pub pic_id: String,
    pub pic_type: String,
    pub url: String,
    pub video_url: String,
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
pub struct Weibo {
    pub text_raw: String,
    pub created_at: DateTime<FixedOffset>,
    pub pics: Vec<WeiboPic>,
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
