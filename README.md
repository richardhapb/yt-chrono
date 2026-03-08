# yt-chrono

Simple CLI to fetch videos from a YouTube channel relative to a root video.

Goal:
- Input a root video URL (or video ID) and `n`.
- Find the channel of that root video.
- Fetch channel videos (including older ones via continuation tokens, not page scroll).
- Save up to `n` videos in forward direction from the root anchor into a text file.

## Requirements

- Rust (with Cargo)
- Internet access

## Run

```bash
cargo run -- "<root_video_url_or_id>" <n> [output.txt]
```

Arguments:
- `root_video_url_or_id`: full URL like `https://www.youtube.com/watch?v=...` or just the 11-char video ID.
- `n`: number of videos to save.
- `output.txt` (optional): output file path. Default is `videos.txt`.

## Example

```bash
# Implementing TCP in Rust (part 1)
cargo r -- 'https://www.youtube.com/watch?v=bzja9fQWzdA' 4 videos.txt
```

Example output file content:

```txt
Implementing TCP in Rust (part 2)
https://www.youtube.com/watch?v=OCpt1I0MWXE

Implementing TCP in Rust (part 3)
https://www.youtube.com/watch?v=8GE6ltLRJA4

Procedural Macros in Rust (part 1)
https://www.youtube.com/watch?v=geovSK3wMB8

Procedural Macros in Rust (part 2)
https://www.youtube.com/watch?v=KVWHT1TAirU
```

## Notes

- This tool does not use browser scrolling.
- It uses YouTube continuation pagination to reach old channel videos.
- If the root video is private/deleted or no longer in the channel list, the command returns an error.
