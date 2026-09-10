use quick_xml::de::from_str;
use regex::Regex;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub text: String,
    pub start: f64,
    pub duration: f64,
}

#[derive(Debug, Deserialize)]
struct TranscriptXML {
    #[serde(rename = "text", default)]
    texts: Vec<TextElement>,
}

#[derive(Debug, Deserialize)]
struct TextElement {
    #[serde(rename = "@start")]
    start: String,
    #[serde(rename = "@dur")]
    dur: String,
    #[serde(rename = "$text", default)]
    content: String,
}

#[derive(Debug)]
pub enum TranscriptError {
    InvalidVideoUrl,
    Request(reqwest::Error),
    HttpStatus(StatusCode),
    Json(serde_json::Error),
    Xml(quick_xml::DeError),
    MissingApiKey,
    Unplayable { status: String, reason: String },
    TranscriptsUnavailable,
    MissingBaseUrl,
    InvalidNumber(std::num::ParseFloatError),
    Io(std::io::Error),
}

impl fmt::Display for TranscriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVideoUrl => write!(f, "invalid youtube url or video id"),
            Self::Request(err) => write!(f, "{err}"),
            Self::HttpStatus(status) => write!(f, "youtube returned HTTP status {status}"),
            Self::Json(err) => write!(f, "{err}"),
            Self::Xml(err) => write!(f, "failed to parse transcript XML: {err}"),
            Self::MissingApiKey => write!(f, "could not find INNERTUBE_API_KEY"),
            Self::Unplayable { status, reason } => {
                write!(f, "video is unplayable: {status} - {reason}")
            }
            Self::TranscriptsUnavailable => {
                write!(f, "transcripts disabled or unavailable")
            }
            Self::MissingBaseUrl => write!(f, "baseUrl not found in caption track"),
            Self::InvalidNumber(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "io error: {err}"),
        }
    }
}

impl Error for TranscriptError {}

impl From<reqwest::Error> for TranscriptError {
    fn from(err: reqwest::Error) -> Self {
        Self::Request(err)
    }
}

impl From<serde_json::Error> for TranscriptError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<quick_xml::DeError> for TranscriptError {
    fn from(err: quick_xml::DeError) -> Self {
        Self::Xml(err)
    }
}

impl From<std::num::ParseFloatError> for TranscriptError {
    fn from(err: std::num::ParseFloatError) -> Self {
        Self::InvalidNumber(err)
    }
}

impl From<std::io::Error> for TranscriptError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

pub fn extract_video_id(input: &str) -> Result<String, TranscriptError> {
    if input.len() == 11
        && input
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Ok(input.to_owned());
    }

    let url = Url::parse(input).map_err(|_| TranscriptError::InvalidVideoUrl)?;

    if url.host_str() == Some("youtu.be") {
        if let Some(id) = url.path_segments().and_then(|mut segments| segments.next()) {
            if id.len() == 11 {
                return Ok(id.to_owned());
            }
        }
    }

    if matches!(
        url.host_str(),
        Some("youtube.com") | Some("www.youtube.com") | Some("m.youtube.com")
    ) {
        if let Some(id) = url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.into_owned())
        {
            if id.len() == 11 {
                return Ok(id);
            }
        }
    }

    Err(TranscriptError::InvalidVideoUrl)
}

pub async fn get_transcript(video_url: &str) -> Result<Vec<Snippet>, TranscriptError> {
    let video_id = extract_video_id(video_url)?;
    let client = Client::new();

    let watch_url = format!("https://www.youtube.com/watch?v={video_id}");

    let response = client
        .get(watch_url)
        .header("Accept-Language", "en-US")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(TranscriptError::HttpStatus(response.status()));
    }

    let html = response.text().await?;

    let api_key_regex =
        Regex::new(r#""INNERTUBE_API_KEY":\s*"([a-zA-Z0-9_-]+)""#).expect("valid regex");

    let api_key = api_key_regex
        .captures(&html)
        .and_then(|captures| captures.get(1))
        .map(|match_| match_.as_str())
        .ok_or(TranscriptError::MissingApiKey)?;

    let payload = json!({
        "context": {
            "client": {
                "clientName": "ANDROID",
                "clientVersion": "20.10.38"
            }
        },
        "videoId": video_id
    });

    let api_url = format!("https://www.youtube.com/youtubei/v1/player?key={api_key}");

    let response = client
        .post(api_url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(TranscriptError::HttpStatus(response.status()));
    }

    let data: Value = response.json().await?;

    if let Some(playability) = data.get("playabilityStatus").and_then(Value::as_object) {
        let status = playability
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();

        if status != "OK" {
            let reason = playability
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or_default();

            return Err(TranscriptError::Unplayable {
                status: status.to_owned(),
                reason: reason.to_owned(),
            });
        }
    }

    let caption_tracks = data
        .get("captions")
        .and_then(|captions| captions.get("playerCaptionsTracklistRenderer"))
        .and_then(|renderer| renderer.get("captionTracks"))
        .and_then(Value::as_array)
        .ok_or(TranscriptError::TranscriptsUnavailable)?;

    let track = caption_tracks
        .first()
        .and_then(Value::as_object)
        .ok_or(TranscriptError::TranscriptsUnavailable)?;

    let base_url = track
        .get("baseUrl")
        .and_then(Value::as_str)
        .ok_or(TranscriptError::MissingBaseUrl)?;

    let base_url = base_url.replace("&fmt=srv3", "");

    let response = client.get(base_url).send().await?;

    if !response.status().is_success() {
        return Err(TranscriptError::HttpStatus(response.status()));
    }

    let xml = response.text().await?;
    let transcript: TranscriptXML = from_str(&xml)?;

    transcript
        .texts
        .into_iter()
        .map(|text| {
            let start = text.start.parse::<f64>()?;
            let duration = text.dur.parse::<f64>()?;

            let content = html_escape::decode_html_entities(&text.content);
            let tags = Regex::new(r"<[^>]*>").expect("valid regex");
            let content = tags.replace_all(&content, "").into_owned();

            Ok(Snippet {
                text: content,
                start,
                duration,
            })
        })
        .collect()
}

pub trait ToSrt {
    fn to_srt_file<P: AsRef<Path>>(self, path: P) -> Result<(), TranscriptError>;
}

fn format_timestamp(seconds: f64) -> String {
    let mut millis = (seconds.fract() * 1000.0).round() as u64;
    let mut total_secs = seconds.trunc() as u64;
    if millis >= 1000 {
        total_secs += millis / 1000;
        millis %= 1000;
    }
    let s = total_secs % 60;
    let m = (total_secs / 60) % 60;
    let h = total_secs / 3600;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, millis)
}

impl ToSrt for Result<Vec<Snippet>, TranscriptError> {
    fn to_srt_file<P: AsRef<Path>>(self, path: P) -> Result<(), TranscriptError> {
        let snippets = self?;
        snippets.to_srt_file(path)
    }
}

impl ToSrt for Vec<Snippet> {
    fn to_srt_file<P: AsRef<Path>>(self, path: P) -> Result<(), TranscriptError> {
        let mut file = File::create(path)?;
        for (i, snippet) in self.iter().enumerate() {
            let start = format_timestamp(snippet.start);
            let end = format_timestamp(snippet.start + snippet.duration);
            writeln!(file, "{}", i + 1)?;
            writeln!(file, "{} --> {}", start, end)?;
            writeln!(file, "{}", snippet.text)?;
            writeln!(file)?;
        }
        Ok(())
    }
}
