
# ytsrt

An asynchronous Rust library for extracting transcripts and captions from YouTube videos.

## Features

* Fully asynchronous API
* Built on Tokio and Reqwest
* Accepts YouTube URLs or video IDs
* Extracts available caption tracks
* Returns transcript text with timestamps and durations
* Simple and lightweight API

## Installation

Add `ytsrt` to your `Cargo.toml`:

```toml
[dependencies]
ytsrt = "0.1"
```

Or install it using Cargo:

```bash
cargo add ytsrt
```

## Usage

```rust
use ytsrt::get_transcript;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transcript =
        get_transcript("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
            .await?;

    for snippet in transcript {
        println!(
            "[{:.2}s] {}",
            snippet.start,
            snippet.text
        );
    }

    Ok(())
}
```

## Video IDs

You can also pass a YouTube video ID directly:

```rust
use ytsrt::get_transcript;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let transcript = get_transcript("dQw4w9WgXcQ").await?;

    for snippet in transcript {
        println!("{}", snippet.text);
    }

    Ok(())
}
```

## Transcript Data

Each transcript entry is returned as a `Snippet`:

```rust
pub struct Snippet {
    pub text: String,
    pub start: f64,
    pub duration: f64,
}
```

* `text` — Caption text
* `start` — Start time in seconds
* `duration` — Duration in seconds

## Error Handling

`get_transcript` returns a `Result` with a `TranscriptError` on failure.

```rust
use ytsrt::get_transcript;

#[tokio::main]
async fn main() {
    match get_transcript("https://www.youtube.com/watch?v=dQw4w9WgXcQ").await {
        Ok(transcript) => {
            for snippet in transcript {
                println!("{}", snippet.text);
            }
        }
        Err(error) => {
            eprintln!("Failed to retrieve transcript: {error}");
        }
    }
}
```

## Requirements

* Rust 1.85 or later
* Tokio runtime

## License

Licensed under the MIT License.
