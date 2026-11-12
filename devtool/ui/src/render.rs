use crate::model::{array, bytes, duration, escape, text};
use serde_json::Value;

fn field(value: &Value, name: &str) -> String {
    escape(text(&value[name]))
}

pub fn settings(snapshot: &Value) -> String {
    let settings = &snapshot["settings"];
    if settings.is_null() {
        return format!(
            "<span class=\"setting\">{}</span>",
            field(snapshot, "settings_error")
        );
    }
    [
        ("parents", &settings["filesystem"]["create_parents"]),
        ("file", &settings["filesystem"]["existing_file"]),
        ("archive", &settings["compression"]["format"]),
        ("level", &settings["compression"]["level"]),
        ("output", &settings["output"]["format"]),
        ("color", &settings["output"]["color"]),
        ("schema", &settings["schema_version"]),
    ]
    .iter()
    .map(|(name, value)| {
        format!(
            "<span class=\"setting\"><span>{name}</span><b>{}</b></span>",
            escape(text(value))
        )
    })
    .collect()
}

pub fn engines(data: &Value, clock: impl Fn(&Value) -> String) -> String {
    array(&data["executables"]).iter().enumerate().map(|(index, engine)| {
        let active = engine["active"].as_bool().unwrap_or(false);
        let watch = engine["name"] == "devtools-watch";
        let note = if active {
            let pids = array(&engine["pids"]).iter().map(text).collect::<Vec<_>>().join(", ");
            format!("PID {pids} · {}", if watch { "achtergrond" } else { "proces actief" })
        } else if engine["last_used"].as_f64().unwrap_or(0.0) > 0.0 {
            format!("Laatst gebruikt {}", clock(&engine["last_used"]))
        } else if watch { "Monitor gestopt".to_string() } else { "Beschikbaar in Bash".to_string() };
        format!("<div class=\"engine-row\" data-active=\"{active}\"><span class=\"engine-index\">{:02}</span><div><strong class=\"engine-name\" title=\"{}\">{}</strong><span class=\"engine-note\">{}</span></div><span class=\"engine-state\">{}</span></div>", index + 1, field(engine, "path"), field(engine, "name"), escape(note), if active { "actief" } else if watch { "uit" } else { "gereed" })
    }).collect()
}

pub fn processes(data: &Value) -> String {
    let rows: String = array(&data["processes"]).iter().map(|process| {
        let shell = process["pid"] == data["shell_pid"];
        let layer = if process["layer"] == "watch" { "Watch-laag" } else if shell { "Bash / PTY" } else { "Bash-child" };
        let foreground = process["tty"].as_i64().unwrap_or(0) != 0 && process["group"] == process["foreground_group"];
        let state = match process["state"].as_str().unwrap_or("") { "R" => "draait", "S" => "wacht", "D" => "I/O", "T" => "gestopt", "I" => "idle", other => other };
        let args = if shell { String::new() } else {
            format!("<code class=\"process-args\" title=\"{}\">{}</code>", field(process, "executable"), escape(array(&process["arguments"]).iter().map(text).collect::<Vec<_>>().join(" ")))
        };
        format!("<div class=\"process-row\"><div class=\"process-top\"><strong>{}</strong><span>PID {}</span></div><div class=\"process-meta\">{layer} · {} · {} · {}</div>{args}</div>", field(process, "name"), field(process, "pid"), if foreground { "voorgrond" } else { "achtergrond" }, escape(state), bytes(process["rss_bytes"].as_u64().unwrap_or(0)))
    }).collect();
    if rows.is_empty() {
        "<p class=\"telemetry-empty\">Geen actieve sessieprocessen gemeten.</p>".to_string()
    } else {
        rows
    }
}

pub fn history(data: &Value, clock: impl Fn(&Value) -> String) -> String {
    let rows: String = array(&data["commands"]).iter().take(30).map(|command| {
        let result = match command["status"].as_str().unwrap_or("") {
            "running" => "draait".to_string(), "interrupted" => "geen eindstatus".to_string(), _ => format!("exit {}", text(&command["exit_code"]))
        };
        let failed = !command["exit_code"].is_null() && command["exit_code"] != 0;
        let executables = array(&command["executables"]).iter().map(text).collect::<Vec<_>>();
        let executables = if executables.is_empty() { "Bash / terminalcommand".to_string() } else { executables.join(" → ") };
        format!("<li class=\"history-entry\" data-status=\"{}\" data-failed=\"{failed}\"><div class=\"history-top\"><time>{}</time><span class=\"history-result\">{} · {}</span></div><code>{}</code><div class=\"history-executables\">{}</div></li>", field(command, "status"), escape(clock(&command["started"])), escape(result), duration(&command["duration_ms"]), field(command, "command"), escape(executables))
    }).collect();
    if rows.is_empty() {
        "<li class=\"telemetry-empty\">Nog geen commands uitgevoerd in deze sessie.</li>"
            .to_string()
    } else {
        rows
    }
}

pub fn files(entries: &[Value], selected: Option<&str>) -> String {
    fn level(
        entries: &[Value],
        parent: &str,
        depth: usize,
        selected: Option<&str>,
        output: &mut String,
    ) {
        let mut children: Vec<_> = entries
            .iter()
            .filter(|entry| {
                entry["path"]
                    .as_str()
                    .unwrap_or("")
                    .rsplit_once('/')
                    .map(|(parent, _)| parent)
                    .unwrap_or("")
                    == parent
            })
            .collect();
        children.sort_by_key(|entry| {
            (
                !entry["directory"].as_bool().unwrap_or(false),
                text(&entry["name"]),
            )
        });
        for entry in children {
            let directory = entry["directory"].as_bool().unwrap_or(false);
            let symlink = entry["symlink"].as_bool().unwrap_or(false);
            let path = entry["path"].as_str().unwrap_or("");
            output.push_str(&format!("<button type=\"button\" class=\"file-row{}\" style=\"padding-inline-start:{}px\" data-file=\"{}\" data-directory=\"{directory}\" data-selected=\"{}\" title=\"{}\"><span class=\"file-symbol\" aria-hidden=\"true\">{}</span><span class=\"file-name\">{}</span>{}</button>", if directory { " directory" } else { "" }, 6 + depth * 13, escape(path), selected == Some(path), escape(path), if directory { "▾" } else if symlink { "↗" } else { "·" }, field(entry, "name"), if directory { String::new() } else { format!("<span class=\"file-size\">{}</span>", bytes(entry["bytes"].as_u64().unwrap_or(0))) }));
            if directory && !symlink {
                level(entries, path, depth + 1, selected, output);
            }
        }
    }
    if entries.is_empty() {
        return "<div class=\"files-empty\"><span aria-hidden=\"true\">└─</span><p>De werkmap is nog leeg.</p><p class=\"muted\">Begin met <code>devtool -D</code>.</p></div>".to_string();
    }
    let mut result = String::new();
    level(entries, "", 0, selected, &mut result);
    result
}

pub fn catalog(schema: &Value) -> String {
    fn walk(command: &Value, prefix: &str, output: &mut Vec<(String, String)>) {
        let path = format!(
            "{}{}{}",
            prefix,
            if prefix.is_empty() { "" } else { " " },
            text(&command["name"])
        );
        if array(&command["commands"]).is_empty() {
            output.push((path, text(&command["about"])));
        } else {
            for child in array(&command["commands"]) {
                walk(child, &path, output);
            }
        }
    }
    let mut commands = Vec::new();
    walk(&schema["cli"], "", &mut commands);
    let mut groups: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for command in commands {
        let name = command.0.split(' ').nth(1).unwrap_or("devtool").to_string();
        match groups.iter_mut().find(|(group, _)| *group == name) {
            Some((_, entries)) => entries.push(command),
            None => groups.push((name, vec![command])),
        }
    }
    groups
        .into_iter()
        .map(|(name, commands)| {
            format!(
                "<section><h2>{}</h2>{}</section>",
                escape(name),
                commands
                    .into_iter()
                    .map(|(path, about)| format!(
                        "<button type=\"button\" data-command=\"{} \" title=\"{}\">{}</button>",
                        escape(&path),
                        escape(about),
                        escape(path)
                    ))
                    .collect::<String>()
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_paths_and_names_remain_literal_in_markup() {
        let entries = vec![
            json!({"path":"<dir>","name":"<dir>","directory":true}),
            json!({"path":"<dir>/\"x.txt","name":"\"x.txt","bytes":7}),
        ];
        let html = files(&entries, Some("<dir>/\"x.txt"));
        assert!(html.contains("data-file=\"&lt;dir&gt;/&quot;x.txt\""));
        assert!(html.contains("data-selected=\"true\""));
        assert_eq!(html.matches("class=\"file-row").count(), 2);
    }

    #[test]
    fn history_preserves_failed_exit_and_command_text() {
        let html = history(
            &json!({"commands":[{"command":"printf '<test>'", "status":"finished", "exit_code":7, "duration_ms":1200, "started":1, "executables":[]}]}),
            |_| "12:00:00".to_string(),
        );
        assert!(html.contains("data-failed=\"true\""));
        assert!(html.contains("exit 7 · 1.2 s"));
        assert!(html.contains("&lt;test&gt;"));
    }
}
