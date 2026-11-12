use serde_json::Value;

pub fn text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null => String::new(),
        value => value.to_string(),
    }
}

pub fn escape(value: impl AsRef<str>) -> String {
    value
        .as_ref()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn compression(source: &str, format: &str, output: &str) -> String {
    let mut parts = vec!["devtool compress".to_string()];
    if !source.is_empty() {
        parts.push(quote(source));
    }
    if !format.is_empty() {
        parts.extend(["--format".to_string(), format.to_string()]);
    }
    if !output.is_empty() {
        parts.extend(["-o".to_string(), quote(output)]);
    }
    parts.join(" ")
}

pub fn bytes(value: u64) -> String {
    if value < 1024 {
        format!("{value} B")
    } else if value < 1024 * 1024 {
        format!("{:.1} K", value as f64 / 1024.0)
    } else {
        format!("{:.1} M", value as f64 / 1024.0 / 1024.0)
    }
}

pub fn duration(value: &Value) -> String {
    match value.as_f64() {
        None => "duur onbekend".to_string(),
        Some(value) if value < 1000.0 => format!("{value:.0} ms"),
        Some(value) => format!("{:.1} s", value / 1000.0),
    }
}

pub fn display_path(path: &str, workspace: &str) -> String {
    if path == workspace {
        "~/workspace".to_string()
    } else if let Some(relative) = path.strip_prefix(&format!("{workspace}/")) {
        format!("~/workspace/{relative}")
    } else {
        path.to_string()
    }
}

pub fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_quotes_shell_paths_without_interpreting_them() {
        assert_eq!(
            compression("/tmp/a'b $(id)", "tar.gz", "out name.gz"),
            "devtool compress '/tmp/a'\\''b $(id)' --format tar.gz -o 'out name.gz'"
        );
        assert_eq!(compression("", "", ""), "devtool compress");
    }

    #[test]
    fn paths_only_shorten_within_the_workspace() {
        assert_eq!(
            display_path("/tmp/work/file", "/tmp/work"),
            "~/workspace/file"
        );
        assert_eq!(
            display_path("/tmp/work-other/file", "/tmp/work"),
            "/tmp/work-other/file"
        );
    }

    #[test]
    fn dynamic_values_are_literal_dom_content() {
        assert_eq!(
            escape("<a title=\"x\">&'"),
            "&lt;a title=&quot;x&quot;&gt;&amp;&#39;"
        );
    }
}
