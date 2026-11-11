//! Utilidades pequenas e sem estado compartilhadas pela TUI e pela CLI.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| home_dir().join(fallback))
}

pub fn config_dir() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("ytui")
}

pub fn data_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join("ytui")
}

pub fn state_dir() -> PathBuf {
    xdg_dir("XDG_STATE_HOME", ".local/state").join("ytui")
}

/// Formata segundos como `m:ss` ou `h:mm:ss`.
pub fn format_duration(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "—".into();
    }
    let total = secs.round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Formata milissegundos como `m:ss`.
pub fn format_duration_ms(ms: u64) -> String {
    format_duration(ms as f64 / 1000.0)
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Corta um texto respeitando caracteres (não bytes) e adiciona reticências.
pub fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn plural(n: usize, singular: &str, plural: &str) -> String {
    format!("{n} {}", if n == 1 { singular } else { plural })
}

pub fn spinner_frame(tick: u64) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(tick as usize) % FRAMES.len()]
}

pub fn tool_available(name: &str, args: &[&str]) -> bool {
    Command::new(name)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Abre uma pasta ou arquivo no gerenciador padrão do sistema.
pub fn open_path(path: &Path) -> Result<()> {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("open", &[])]
    } else if cfg!(target_os = "windows") {
        &[("explorer", &[])]
    } else {
        &[("xdg-open", &[]), ("gio", &["open"])]
    };
    for (cmd, args) in candidates {
        if Command::new(cmd)
            .args(*args)
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }
    bail!("nenhum abridor de arquivos disponível (xdg-open/gio)")
}

/// Notificação desktop opcional; falha silenciosamente quando não há suporte.
pub fn desktop_notification(summary: &str, body: &str) {
    if cfg!(target_os = "macos") {
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            body.replace('"', "'"),
            summary.replace('"', "'")
        );
        let _ = Command::new("osascript")
            .args(["-e", &script])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        return;
    }
    if !tool_available("notify-send", &["--help"]) {
        return;
    }
    let _ = Command::new("notify-send")
        .args(["-a", "ytui", "-u", "normal", summary, body])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Interpreta listas 1-based como `1,3-5,8` (usadas por `--playlist-items`).
pub fn parse_selection(input: &str, max: usize) -> Result<Vec<usize>> {
    let mut items = Vec::new();
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            let start: usize = a
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("faixa inválida: {part}"))?;
            let end: usize = b
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("faixa inválida: {part}"))?;
            if start == 0 || end < start {
                bail!("intervalo inválido: {part}");
            }
            if max > 0 && end > max {
                bail!("intervalo {part} ultrapassa o total de {max} faixas");
            }
            items.extend(start..=end);
        } else {
            let n: usize = part
                .parse()
                .map_err(|_| anyhow::anyhow!("faixa inválida: {part}"))?;
            if n == 0 {
                bail!("faixas são numeradas a partir de 1");
            }
            if max > 0 && n > max {
                bail!("faixa {n} ultrapassa o total de {max}");
            }
            items.push(n);
        }
    }
    items.sort_unstable();
    items.dedup();
    if items.is_empty() {
        bail!("seleção vazia");
    }
    Ok(items)
}

/// Compacta índices 1-based em intervalos: `[1,2,3,5]` → `1-3,5`.
pub fn format_ranges(indices: &[usize]) -> String {
    if indices.is_empty() {
        return String::new();
    }
    let mut sorted = indices.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut parts: Vec<String> = Vec::new();
    let mut start = sorted[0];
    let mut prev = sorted[0];
    for &n in sorted.iter().skip(1) {
        if n == prev + 1 {
            prev = n;
            continue;
        }
        parts.push(range_text(start, prev));
        start = n;
        prev = n;
    }
    parts.push(range_text(start, prev));
    parts.join(",")
}

fn range_text(start: usize, end: usize) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

/// Extrai a primeira URL de um texto colado e remove espaços acidentais.
pub fn extract_url(text: &str) -> String {
    let trimmed = text.trim();
    for token in trimmed.split_whitespace() {
        if token.starts_with("http://")
            || token.starts_with("https://")
            || token.starts_with("spotify:")
        {
            return token.trim_matches(|c| c == '"' || c == '\'').to_string();
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(format_duration(0.0), "0:00");
        assert_eq!(format_duration(61.2), "1:01");
        assert_eq!(format_duration(3723.0), "1:02:03");
        assert_eq!(format_duration(-1.0), "—");
    }

    #[test]
    fn ranges_roundtrip() {
        let items = parse_selection("1,3-5,8", 10).unwrap();
        assert_eq!(items, vec![1, 3, 4, 5, 8]);
        assert_eq!(format_ranges(&items), "1,3-5,8");
        assert_eq!(format_ranges(&[]), "");
        assert!(parse_selection("0", 10).is_err());
        assert!(parse_selection("9-12", 10).is_err());
    }

    #[test]
    fn truncation_is_char_safe() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("ação", 10), "ação");
    }

    #[test]
    fn url_extraction() {
        assert_eq!(extract_url("  https://x.test/a  "), "https://x.test/a");
        assert_eq!(
            extract_url("veja: https://open.spotify.com/track/x?si=1 fim"),
            "https://open.spotify.com/track/x?si=1"
        );
    }
}
