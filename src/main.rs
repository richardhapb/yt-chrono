use anyhow::{Context, Result, anyhow};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::env;
use std::fs;

const YT_BASE: &str = "https://www.youtube.com";
const BROWSE_ENDPOINT: &str = "https://www.youtube.com/youtubei/v1/browse";

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: yt-chrono <root-video-url-or-id> <n> [output.txt]");
        eprintln!(
            "Example: yt-chrono \"https://www.youtube.com/watch?v=dQw4w9WgXcQ\" 25 videos.txt"
        );
        return Ok(());
    }

    let root_video_id = extract_video_id(&args[1])?;
    let n: usize = args[2]
        .parse()
        .with_context(|| format!("Invalid n value: {}", args[2]))?;
    let output_path = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "videos.txt".to_string());

    let client = build_client()?;

    let root_html = fetch_text(&client, &format!("{YT_BASE}/watch?v={root_video_id}"))?;
    let channel_id = extract_channel_id(&root_html)?;

    let channel_videos_html =
        fetch_text(&client, &format!("{YT_BASE}/channel/{channel_id}/videos"))?;
    let ytcfg_json = extract_json_after_marker(&channel_videos_html, "ytcfg.set({")
        .ok_or_else(|| anyhow!("Could not find ytcfg.set JSON"))?;
    let ytcfg: Value = serde_json::from_str(&ytcfg_json).context("Invalid ytcfg JSON")?;
    let api_key = ytcfg
        .get("INNERTUBE_API_KEY")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("INNERTUBE_API_KEY not found"))?;
    let context = ytcfg
        .get("INNERTUBE_CONTEXT")
        .cloned()
        .ok_or_else(|| anyhow!("INNERTUBE_CONTEXT not found"))?;

    let initial_data_json = extract_json_after_marker(&channel_videos_html, "var ytInitialData = ")
        .ok_or_else(|| anyhow!("Could not find ytInitialData JSON"))?;
    let initial_data: Value =
        serde_json::from_str(&initial_data_json).context("Invalid ytInitialData JSON")?;

    let mut collected: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let (initial_ids, mut continuation) = extract_initial_ids_and_continuation(&initial_data);
    push_unique_in_order(&mut collected, &mut seen, initial_ids);

    while !collected.iter().any(|id| id == &root_video_id) {
        let Some(token) = continuation else {
            break;
        };
        let response = fetch_continuation(&client, api_key, &context, &token)?;
        let (ids, next) = extract_continuation_ids_and_token(&response);
        if ids.is_empty() && next.is_none() {
            break;
        }
        push_unique_in_order(&mut collected, &mut seen, ids);
        continuation = next;
    }

    let Some(root_index) = collected.iter().position(|id| id == &root_video_id) else {
        return Err(anyhow!(
            "Root video ({root_video_id}) not found in channel list. It may be private/deleted or the channel has changed."
        ));
    };

    let result: Vec<String> = collected[..root_index]
        .iter()
        .rev()
        .take(n)
        .cloned()
        .collect();

    let output = if result.is_empty() {
        String::new()
    } else {
        result
            .into_iter()
            .map(|id| format!("{YT_BASE}/watch?v={id}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    fs::write(&output_path, output).with_context(|| format!("Failed to write {output_path}"))?;

    println!("Saved up to {n} videos after root video timestamp anchor into {output_path}");
    Ok(())
}

fn build_client() -> Result<Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36",
        ),
    );
    Client::builder()
        .default_headers(headers)
        .build()
        .context("Failed to build HTTP client")
}

fn fetch_text(client: &Client, url: &str) -> Result<String> {
    client
        .get(url)
        .send()
        .with_context(|| format!("GET failed: {url}"))?
        .error_for_status()
        .with_context(|| format!("Bad HTTP status: {url}"))?
        .text()
        .context("Failed to read response body")
}

fn fetch_continuation(
    client: &Client,
    api_key: &str,
    context: &Value,
    token: &str,
) -> Result<Value> {
    let url = format!("{BROWSE_ENDPOINT}?key={api_key}");
    let payload = json!({
        "context": context,
        "continuation": token
    });

    client
        .post(url)
        .json(&payload)
        .send()
        .context("Failed to call browse continuation endpoint")?
        .error_for_status()
        .context("Continuation endpoint returned HTTP error")?
        .json()
        .context("Failed to parse continuation JSON")
}

fn extract_video_id(input: &str) -> Result<String> {
    if input.len() == 11 && !input.contains('/') && !input.contains('?') {
        return Ok(input.to_string());
    }

    if let Some(pos) = input.find("v=") {
        let rest = &input[pos + 2..];
        let id = rest.split('&').next().unwrap_or(rest);
        if id.len() == 11 {
            return Ok(id.to_string());
        }
    }

    if let Some(pos) = input.find("youtu.be/") {
        let rest = &input[pos + "youtu.be/".len()..];
        let id = rest.split('?').next().unwrap_or(rest);
        if id.len() == 11 {
            return Ok(id.to_string());
        }
    }

    Err(anyhow!(
        "Could not extract a valid 11-char YouTube video ID from: {input}"
    ))
}

fn extract_channel_id(html: &str) -> Result<String> {
    let marker = "\"channelId\":\"";
    let idx = html
        .find(marker)
        .ok_or_else(|| anyhow!("channelId not found in root video page"))?;
    let rest = &html[idx + marker.len()..];
    let end = rest
        .find('"')
        .ok_or_else(|| anyhow!("Malformed channelId field"))?;
    Ok(rest[..end].to_string())
}

fn extract_json_after_marker(haystack: &str, marker: &str) -> Option<String> {
    let start = haystack.find(marker)?;
    let json_start = if marker.ends_with('{') {
        start + marker.len() - 1
    } else {
        let after = &haystack[start + marker.len()..];
        let brace_start = after.find('{')?;
        start + marker.len() + brace_start
    };
    let chars: Vec<char> = haystack[json_start..].chars().collect();

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (i, ch) in chars.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *ch == '\\' {
                escaped = true;
            } else if *ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let json_text: String = chars[..=i].iter().collect();
                    return Some(json_text);
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_initial_ids_and_continuation(data: &Value) -> (Vec<String>, Option<String>) {
    let tabs = data
        .get("contents")
        .and_then(|v| v.get("twoColumnBrowseResultsRenderer"))
        .and_then(|v| v.get("tabs"))
        .and_then(Value::as_array);

    let Some(tabs) = tabs else {
        return (Vec::new(), None);
    };

    for tab in tabs {
        let tab_renderer = match tab.get("tabRenderer") {
            Some(v) => v,
            None => continue,
        };

        if tab_renderer.get("selected").and_then(Value::as_bool) != Some(true) {
            continue;
        }

        let contents = tab_renderer
            .get("content")
            .and_then(|v| v.get("richGridRenderer"))
            .and_then(|v| v.get("contents"))
            .and_then(Value::as_array);
        let Some(contents) = contents else {
            continue;
        };
        return extract_ids_and_token_from_items(contents);
    }

    (Vec::new(), None)
}

fn extract_continuation_ids_and_token(data: &Value) -> (Vec<String>, Option<String>) {
    let mut items_ref: Option<&Vec<Value>> = None;

    if let Some(actions) = data
        .get("onResponseReceivedActions")
        .and_then(Value::as_array)
    {
        for action in actions {
            if let Some(items) = action
                .get("appendContinuationItemsAction")
                .and_then(|v| v.get("continuationItems"))
                .and_then(Value::as_array)
            {
                items_ref = Some(items);
                break;
            }
        }
    }

    if items_ref.is_none()
        && let Some(actions) = data
            .get("onResponseReceivedEndpoints")
            .and_then(Value::as_array)
    {
        for action in actions {
            if let Some(items) = action
                .get("appendContinuationItemsAction")
                .and_then(|v| v.get("continuationItems"))
                .and_then(Value::as_array)
            {
                items_ref = Some(items);
                break;
            }
        }
    }

    match items_ref {
        Some(items) => extract_ids_and_token_from_items(items),
        None => (Vec::new(), None),
    }
}

fn extract_ids_and_token_from_items(items: &[Value]) -> (Vec<String>, Option<String>) {
    let mut ids = Vec::new();
    let mut token = None;

    for item in items {
        if let Some(video_id) = item
            .get("richItemRenderer")
            .and_then(|v| v.get("content"))
            .and_then(|v| v.get("videoRenderer"))
            .and_then(|v| v.get("videoId"))
            .and_then(Value::as_str)
        {
            ids.push(video_id.to_string());
            continue;
        }

        if let Some(video_id) = item
            .get("gridVideoRenderer")
            .and_then(|v| v.get("videoId"))
            .and_then(Value::as_str)
        {
            ids.push(video_id.to_string());
            continue;
        }

        if token.is_none() {
            token = item
                .get("continuationItemRenderer")
                .and_then(|v| v.get("continuationEndpoint"))
                .and_then(|v| v.get("continuationCommand"))
                .and_then(|v| v.get("token"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
        }
    }

    (ids, token)
}

fn push_unique_in_order(
    target: &mut Vec<String>,
    seen: &mut HashSet<String>,
    incoming: Vec<String>,
) {
    for id in incoming {
        if seen.insert(id.clone()) {
            target.push(id);
        }
    }
}
