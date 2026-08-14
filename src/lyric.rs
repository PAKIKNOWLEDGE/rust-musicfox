//! LRC lyric parsing.

use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct LyricLine {
    pub time_ms: u64,
    pub text: String,
}

/// Parse LRC text into sorted lyric lines.
pub fn parse_lrc(input: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in input.lines() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        // Collect every [mm:ss.xx] tag, then the text after the last tag.
        let mut idx = 0usize;
        let mut times = Vec::new();
        while let Some(open) = raw[idx..].find('[') {
            let open = idx + open;
            let Some(close) = raw[open..].find(']') else {
                break;
            };
            let close = open + close;
            let tag = &raw[open + 1..close];
            if let Some(ms) = parse_time(tag) {
                times.push(ms);
            }
            idx = close + 1;
        }
        if times.is_empty() {
            continue;
        }
        let text = raw[idx..].trim().to_string();
        for t in times {
            lines.push(LyricLine {
                time_ms: t,
                text: text.clone(),
            });
        }
    }
    lines.sort_by_key(|l| l.time_ms);
    lines
}

/// Parse a `mm:ss` or `mm:ss.xx` or `mm:ss:xx` tag into milliseconds.
fn parse_time(tag: &str) -> Option<u64> {
    let parts: Vec<&str> = tag.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let min: u64 = parts[0].trim().parse().ok()?;
    let sec_str = parts[1].trim();
    // Allow seconds or seconds.fraction
    let sec_part = sec_str.split('.').next().unwrap_or(sec_str);
    let sec: u64 = sec_part.parse().ok()?;
    let mut ms = min * 60_000 + sec * 1000;
    if let Some(frac) = sec_str.split_once('.') {
        let frac: u64 = frac.1.chars().take(3).collect::<String>().parse().ok()?;
        let frac = if frac < 100 { frac * 10 } else { frac };
        ms += frac;
    }
    Some(ms)
}

/// Index of the lyric line active at `pos`, or None when out of range.
pub fn current_index(lines: &[LyricLine], pos: Duration) -> Option<usize> {
    if lines.is_empty() {
        return None;
    }
    let ms = pos.as_millis() as u64;
    let mut cur = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if line.time_ms <= ms {
            cur = i;
        } else {
            break;
        }
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let lrc = "[00:12.34]hello\n[00:15.00]world\n";
        let lines = parse_lrc(lrc);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].time_ms, 12_340);
        assert_eq!(lines[0].text, "hello");
        assert_eq!(lines[1].time_ms, 15_000);
    }

    #[test]
    fn parse_multi_tag() {
        let lrc = "[00:01.00][00:03.00]repeat\n";
        let lines = parse_lrc(lrc);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "repeat");
        assert_eq!(lines[1].time_ms, 3_000);
    }

    #[test]
    fn parse_meta_ignored() {
        let lrc = "[ti:test]\n[ar:artist]\n[00:01.00]ok\n";
        let lines = parse_lrc(lrc);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "ok");
    }

    #[test]
    fn current_line() {
        let lines = parse_lrc("[00:01.00]a\n[00:05.00]b\n");
        assert_eq!(current_index(&lines, Duration::from_millis(0)), Some(0));
        assert_eq!(current_index(&lines, Duration::from_millis(4_999)), Some(0));
        assert_eq!(current_index(&lines, Duration::from_millis(5_000)), Some(1));
        assert_eq!(
            current_index(&lines, Duration::from_millis(99_000)),
            Some(1)
        );
    }
}
