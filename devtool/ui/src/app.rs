use crate::{model::*, render};
use base64::{engine::general_purpose::STANDARD, Engine};
use js_sys::{Array, Date, Function, Uint8Array};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::VecDeque};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    Element, Event, HtmlElement, HtmlInputElement, HtmlSelectElement, RequestInit, Response,
};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = DevtoolsTerminal, js_name = create, catch)]
    fn terminal_create(data: &Function, resize: &Function) -> Result<(), JsValue>;
    #[wasm_bindgen(js_namespace = DevtoolsTerminal, js_name = write)]
    fn terminal_write(bytes: &Uint8Array);
    #[wasm_bindgen(js_namespace = DevtoolsTerminal, js_name = reset)]
    fn terminal_reset();
    #[wasm_bindgen(js_namespace = DevtoolsTerminal, js_name = focus)]
    fn terminal_focus();
    #[wasm_bindgen(js_namespace = DevtoolsTerminal, js_name = fit)]
    fn terminal_fit() -> Array;
}

#[derive(Default)]
struct State {
    session: Option<String>,
    cursor: u64,
    workspace: String,
    ready: bool,
    starting: bool,
    output_busy: bool,
    refresh_busy: bool,
    telemetry_busy: bool,
    input_busy: bool,
    input: VecDeque<(String, String)>,
    settings_signature: String,
    files_signature: String,
    telemetry_signature: String,
    entries: Vec<Value>,
    selected: Option<String>,
}

thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

fn state<T>(function: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|state| function(&mut state.borrow_mut()))
}
fn window() -> web_sys::Window {
    web_sys::window().unwrap()
}
fn document() -> web_sys::Document {
    window().document().unwrap()
}
fn element(id: &str) -> Element {
    document().get_element_by_id(id).unwrap()
}
fn set_text(id: &str, value: &str) {
    let target = element(id);
    if target.text_content().as_deref() != Some(value) {
        target.set_text_content(Some(value));
    }
}
fn html(id: &str, value: &str) {
    element(id).set_inner_html(value);
}
fn attribute(id: &str, name: &str, value: &str) {
    let _ = element(id).set_attribute(name, value);
}
fn hidden(id: &str, value: bool) {
    element(id)
        .unchecked_into::<HtmlElement>()
        .set_hidden(value);
}
fn value(id: &str) -> String {
    let target = element(id);
    if let Some(input) = target.dyn_ref::<HtmlInputElement>() {
        input.value()
    } else {
        target.unchecked_into::<HtmlSelectElement>().value()
    }
}
fn input_value(id: &str, value: &str) {
    element(id)
        .unchecked_into::<HtmlInputElement>()
        .set_value(value);
}
fn error_text(error: JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&error, &"message".into())
                .ok()
                .and_then(|value| value.as_string())
        })
        .unwrap_or_else(|| format!("{error:?}"))
}
fn report(error: JsValue) {
    set_text("activity", &error_text(error));
}
fn connection(label: &str, kind: &str) {
    set_text("connection", label);
    attribute("connection", "data-state", kind);
}
fn current(session: &str) -> bool {
    state(|state| state.session.as_deref() == Some(session))
}
fn session() -> Option<String> {
    state(|state| state.session.clone())
}
fn ready(ready: bool, alive: bool) {
    let enabled = ready && alive && !state(|state| state.starting);
    state(|state| state.ready = enabled);
    if let Ok(nodes) =
        document().query_selector_all("[data-command], .catalog button, #compression-form button")
    {
        for index in 0..nodes.length() {
            let button: Element = nodes.item(index).unwrap().unchecked_into();
            if enabled {
                let _ = button.remove_attribute("disabled");
            } else {
                let _ = button.set_attribute("disabled", "");
            }
        }
    }
    if !alive {
        connection("Shell beëindigd", "error");
        set_text(
            "activity",
            "De shell is afgesloten. Nieuwe sessie opent een nieuwe terminal.",
        );
    }
}

async fn response(path: &str, body: Option<Value>) -> Result<Response, JsValue> {
    let options = RequestInit::new();
    let headers = web_sys::Headers::new()?;
    if let Some(body) = body {
        options.set_method("POST");
        options.set_body(&JsValue::from_str(&body.to_string()));
        headers.set("Content-Type", "application/json")?;
    }
    options.set_headers(&headers);
    let response: Response = JsFuture::from(window().fetch_with_str_and_init(path, &options))
        .await?
        .dyn_into()?;
    Ok(response)
}

async fn api(path: &str, body: Option<Value>) -> Result<Value, JsValue> {
    let response = response(path, body).await?;
    let raw = JsFuture::from(response.text()?)
        .await?
        .as_string()
        .unwrap_or_default();
    let value: Value =
        serde_json::from_str(&raw).map_err(|error| JsValue::from_str(&error.to_string()))?;
    if !response.ok() {
        return Err(JsValue::from_str(
            &value["error"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("HTTP {}", response.status())),
        ));
    }
    Ok(value)
}

fn send(input: String) {
    let start = state(|state| {
        if let Some(session) = &state.session {
            state.input.push_back((session.clone(), input));
        }
        if state.input_busy || state.input.is_empty() {
            false
        } else {
            state.input_busy = true;
            true
        }
    });
    if start {
        spawn_local(async {
            loop {
                let next = state(|state| state.input.pop_front());
                let Some((session, input)) = next else {
                    state(|state| state.input_busy = false);
                    break;
                };
                if !current(&session) {
                    continue;
                }
                if let Err(error) = api(
                    "/api/input",
                    Some(json!({"session": session, "data":STANDARD.encode(input.as_bytes())})),
                )
                .await
                {
                    if current(&session) {
                        report(error);
                        connection("Verbinding verbroken", "error");
                    }
                }
            }
        });
    }
}

fn prepare(command: String) {
    if !state(|state| state.ready) {
        return;
    }
    send(command);
    terminal_focus();
    set_text("activity", "Opdracht ingevoegd · Enter voert uit");
}
fn compression() -> String {
    crate::model::compression(
        &value("compression-source"),
        &value("compression-format"),
        &value("compression-output"),
    )
}
fn update_compression() {
    set_text("compression-preview", &compression());
}
fn resize(columns: u32, rows: u32) {
    set_text("terminal-size", &format!("{columns} × {rows} · BASH PTY"));
    if let Some(session) = session() {
        spawn_local(async move {
            if let Err(error) = api(
                "/api/resize",
                Some(json!({"session":session,"columns":columns,"rows":rows})),
            )
            .await
            {
                report(error);
            }
        });
    }
}
fn fit() {
    let size = terminal_fit();
    resize(
        size.get(0).as_f64().unwrap_or(80.0) as u32,
        size.get(1).as_f64().unwrap_or(24.0) as u32,
    );
}
fn toggle(panel: &str, button: &str, force: Option<bool>) {
    let open = force.unwrap_or_else(|| element(panel).unchecked_into::<HtmlElement>().hidden());
    hidden(panel, !open);
    attribute(button, "aria-expanded", &open.to_string());
    fit();
}
fn inspector(telemetry: bool) {
    hidden("telemetry-panel", !telemetry);
    hidden("file-panel", telemetry);
    attribute("telemetry-tab", "aria-pressed", &telemetry.to_string());
    attribute("files-tab", "aria-pressed", &(!telemetry).to_string());
    fit();
}
fn clock(value: &Value) -> String {
    Date::new(&JsValue::from_f64(value.as_f64().unwrap_or(0.0) * 1000.0))
        .to_locale_time_string("nl-BE")
        .into()
}

fn render_telemetry(data: &Value) {
    let alive = data["shell_alive"].as_bool().unwrap_or(false);
    let stale = alive && Date::now() / 1000.0 - data["sampled_at"].as_f64().unwrap_or(0.0) > 3.0;
    let complete = data["coverage"] == "complete";
    attribute(
        "telemetry-status",
        "data-state",
        if stale || !complete {
            "error"
        } else if alive {
            "live"
        } else {
            "closed"
        },
    );
    set_text(
        "telemetry-status",
        if stale {
            "Verouderd"
        } else if !complete {
            "Beperkte meting"
        } else if alive {
            "Live"
        } else {
            "Afgesloten"
        },
    );
    attribute("telemetry-status", "title", "");
    set_text(
        "telemetry-updated",
        &if data["sampled_at"].as_f64().unwrap_or(0.0) > 0.0 {
            clock(&data["sampled_at"])
        } else {
            "Nog geen meting".to_string()
        },
    );
    set_text(
        "telemetry-source-label",
        if array(&data["sources"]).iter().any(|source| source == "ps") {
            "Bash-events + macOS ps"
        } else {
            "Bash-events + /proc"
        },
    );
    let signature = json!([
        data["processes"],
        data["commands"],
        data["executables"],
        data["shell_ready"],
        data["shell_alive"],
        data["watch_running"]
    ])
    .to_string();
    if !state(|state| {
        if signature == state.telemetry_signature {
            false
        } else {
            state.telemetry_signature = signature;
            true
        }
    }) {
        return;
    }
    let command = array(&data["commands"])
        .iter()
        .find(|command| command["status"] == "running");
    set_text(
        "telemetry-mode",
        if !alive {
            "Afgesloten"
        } else if command.is_some() {
            "Command draait"
        } else if data["shell_ready"] == true {
            "Prompt gereed"
        } else {
            "Bash actief"
        },
    );
    set_text(
        "telemetry-current-command",
        &command
            .map(|command| text(&command["command"]))
            .unwrap_or_else(|| {
                if alive {
                    "Bash wacht op een command".to_string()
                } else {
                    "Bash is afgesloten".to_string()
                }
            }),
    );
    html(
        "telemetry-current-meta",
        &format!(
            "<span>Bash PID {}</span><span>{}</span>{}",
            escape(text(&data["shell_pid"])),
            if data["watch_running"] == true {
                "Watch actief"
            } else {
                "Watch uit"
            },
            command
                .map(|command| format!("<span>{}</span>", duration(&command["duration_ms"])))
                .unwrap_or_default()
        ),
    );
    html("telemetry-engines", &render::engines(data, clock));
    html("telemetry-processes", &render::processes(data));
    html("telemetry-history", &render::history(data, clock));
    set_text(
        "telemetry-process-count",
        &format!("{} actief", array(&data["processes"]).len()),
    );
    set_text(
        "telemetry-command-count",
        &format!(
            "{} / {}",
            array(&data["commands"]).len(),
            text(&data["history_limit"])
        ),
    );
}

fn refresh_telemetry() {
    let next = state(|state| {
        if state.telemetry_busy {
            None
        } else {
            state
                .session
                .clone()
                .inspect(|_| state.telemetry_busy = true)
        }
    });
    if let Some(session) = next {
        spawn_local(async move {
            match api(&format!("/api/telemetry?session={session}"), None).await {
                Ok(data) if current(&session) => render_telemetry(&data),
                Err(error) if current(&session) => {
                    set_text("telemetry-status", "Geen verbinding");
                    attribute("telemetry-status", "data-state", "error");
                    attribute("telemetry-status", "title", &error_text(error));
                }
                _ => {}
            }
            state(|state| state.telemetry_busy = false);
        });
    }
}

fn refresh() {
    let next = state(|state| {
        if state.refresh_busy {
            None
        } else {
            state.session.clone().inspect(|_| state.refresh_busy = true)
        }
    });
    if let Some(session) = next {
        spawn_local(async move {
            let result = async {
                let snapshot = api(&format!("/api/state?session={session}"), None).await?;
                let files = api(&format!("/api/files?session={session}"), None).await?;
                if !current(&session) {
                    return Ok::<_, JsValue>(());
                }
                let workspace = text(&snapshot["workspace"]);
                set_text("cwd", &display_path(&text(&snapshot["cwd"]), &workspace));
                attribute("cwd", "title", &text(&snapshot["cwd"]));
                set_text(
                    "run-status",
                    &snapshot["last_run"]
                        .as_u64()
                        .map(|run| format!("run-nr.{run:03}"))
                        .unwrap_or_else(|| "Nog geen hoofdmap".to_string()),
                );
                let signature =
                    json!([snapshot["settings"], snapshot["settings_error"]]).to_string();
                let settings_changed = state(|state| {
                    state.workspace = workspace;
                    if state.settings_signature == signature {
                        false
                    } else {
                        state.settings_signature = signature;
                        true
                    }
                });
                if settings_changed {
                    html("settings-values", &render::settings(&snapshot));
                    attribute("settings-values", "title", &text(&snapshot["config_path"]));
                }
                let signature = files["entries"].to_string();
                let entries = array(&files["entries"]).to_vec();
                let selected = state(|state| state.selected.clone());
                let files_changed = state(|state| {
                    state.entries = entries.clone();
                    if state.files_signature == signature {
                        false
                    } else {
                        state.files_signature = signature;
                        true
                    }
                });
                if files_changed {
                    set_text(
                        "file-count",
                        &format!(
                            "{} bestanden",
                            entries
                                .iter()
                                .filter(|entry| entry["directory"] != true)
                                .count()
                        ),
                    );
                    html("file-tree", &render::files(&entries, selected.as_deref()));
                }
                ready(snapshot["ready"] == true, snapshot["alive"] == true);
                Ok(())
            }
            .await;
            if current(&session) {
                if let Err(error) = result {
                    report(error);
                }
            }
            state(|state| state.refresh_busy = false);
        });
    }
}

fn output() {
    let next = state(|state| {
        if state.output_busy {
            None
        } else {
            state.session.clone().map(|session| {
                state.output_busy = true;
                (session, state.cursor)
            })
        }
    });
    if let Some((session, cursor)) = next {
        spawn_local(async move {
            match api(
                &format!("/api/output?session={session}&cursor={cursor}"),
                None,
            )
            .await
            {
                Ok(output) if current(&session) => {
                    if output["reset"] == true {
                        terminal_reset();
                    }
                    if let Some(encoded) = output["data"].as_str() {
                        if let Ok(bytes) = STANDARD.decode(encoded) {
                            terminal_write(&Uint8Array::from(bytes.as_slice()));
                        }
                    }
                    state(|state| state.cursor = output["cursor"].as_u64().unwrap_or(cursor));
                    ready(output["ready"] == true, output["alive"] == true);
                    if output["alive"] == true {
                        connection("Verbonden", "connected");
                    }
                }
                Err(error) if current(&session) => {
                    connection("Opnieuw verbinden…", "error");
                    report(error);
                }
                _ => {}
            }
            state(|state| state.output_busy = false);
        });
    }
}

async fn start_session(existing: Option<String>) -> Result<(), JsValue> {
    if state(|state| {
        if state.starting {
            true
        } else {
            state.starting = true;
            false
        }
    }) {
        return Ok(());
    }
    ready(false, true);
    let result = api(
        "/api/session",
        Some(
            existing
                .map(|session| json!({"session":session}))
                .unwrap_or_else(|| json!({})),
        ),
    )
    .await;
    state(|state| state.starting = false);
    let snapshot = result?;
    let id = text(&snapshot["id"]);
    state(|state| {
        state.session = Some(id.clone());
        state.cursor = 0;
        state.workspace = text(&snapshot["workspace"]);
        state.settings_signature.clear();
        state.files_signature.clear();
        state.telemetry_signature.clear();
        state.entries.clear();
        state.selected = None;
        state.input.clear();
    });
    if let Ok(Some(storage)) = window().local_storage() {
        let _ = storage.set_item("devtool-terminal-session", &id);
    }
    set_text("session-label", &id.chars().take(6).collect::<String>());
    attribute("session-label", "title", &text(&snapshot["workspace"]));
    terminal_reset();
    hidden("file-preview", true);
    connection("Verbonden", "connected");
    set_text("activity", "Rust/WASM-interface gereed");
    refresh();
    refresh_telemetry();
    fit();
    Ok(())
}

fn preview(path: String) {
    let Some(session) = session() else {
        return;
    };
    let entry = state(|state| {
        state
            .entries
            .iter()
            .find(|entry| entry["path"] == path)
            .cloned()
    });
    let Some(entry) = entry else {
        return;
    };
    state(|state| {
        state.selected = Some(path.clone());
        state.files_signature.clear();
    });
    let name = text(&entry["name"]);
    set_text("preview-name", &name);
    let encoded = js_sys::encode_uri_component(&path)
        .as_string()
        .unwrap_or_default();
    let url = format!("/api/file?session={session}&path={encoded}");
    attribute("download-file", "href", &url);
    hidden("file-preview", false);
    if name.to_lowercase().ends_with(".pdf") {
        html(
            "preview-content",
            &format!(
                "<iframe src=\"{}\" title=\"{}\"></iframe>",
                escape(url),
                escape(name)
            ),
        );
        return;
    }
    set_text("preview-content", "Laden…");
    spawn_local(async move {
        let result = async {
            let response = response(&url, None).await?;
            if !response.ok() {
                return Err(JsValue::from_str(&format!("HTTP {}", response.status())));
            }
            let bytes = Uint8Array::new(&JsFuture::from(response.array_buffer()?).await?).to_vec();
            let binary = name.ends_with(".7z")
                || name.ends_with(".gz")
                || bytes.iter().take(2048).any(|byte| *byte == 0);
            let content = if binary {
                format!(
                    "{} bytes\n\n{}",
                    bytes.len(),
                    bytes
                        .iter()
                        .take(256)
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            } else if bytes.is_empty() {
                "(leeg bestand)".to_string()
            } else {
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&bytes[..bytes.len().min(16384)]),
                    if bytes.len() > 16384 {
                        format!("\n\n… voorbeeld van 16384 / {} bytes", bytes.len())
                    } else {
                        String::new()
                    }
                )
            };
            Ok(content)
        }
        .await;
        if current(&session) && state(|state| state.selected.as_deref() == Some(&path)) {
            match result {
                Ok(content) => html(
                    "preview-content",
                    &format!("<pre>{}</pre>", escape(content)),
                ),
                Err(error) => set_text("preview-content", &error_text(error)),
            }
        }
    });
}

fn bind(event: &str, callback: impl FnMut(Event) + 'static) -> Result<(), JsValue> {
    let callback = Closure::<dyn FnMut(Event)>::new(callback);
    document().add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())?;
    callback.forget();
    Ok(())
}

fn controls() -> Result<(), JsValue> {
    bind("click", |event| {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
        else {
            return;
        };
        let Some(button) = target.closest("button").ok().flatten() else {
            return;
        };
        if button.has_attribute("disabled") {
            return;
        }
        if let Some(command) = button.get_attribute("data-command") {
            prepare(command);
            return;
        }
        if let Some(key) = button.get_attribute("data-key") {
            send(
                match key.as_str() {
                    "tab" => "\t",
                    "up" => "\x1b[A",
                    "down" => "\x1b[B",
                    "interrupt" => "\x03",
                    "eof" => "\x04",
                    _ => "",
                }
                .to_string(),
            );
            terminal_focus();
            return;
        }
        if let Some(path) = button.get_attribute("data-file") {
            if button.get_attribute("data-directory").as_deref() == Some("true") {
                let workspace = state(|state| state.workspace.clone());
                input_value("compression-source", &format!("{workspace}/{path}"));
                update_compression();
                toggle("compression-panel", "compression-toggle", Some(true));
            } else {
                preview(path);
            }
            return;
        }
        match button.id().as_str() {
            "compression-toggle" => toggle("compression-panel", "compression-toggle", None),
            "catalog-toggle" => toggle("catalog-panel", "catalog-toggle", None),
            "new-session" => spawn_local(async {
                if let Err(error) = start_session(None).await {
                    report(error);
                }
            }),
            "refresh-files" => {
                state(|state| state.files_signature.clear());
                refresh();
            }
            "close-preview" => {
                hidden("file-preview", true);
                state(|state| state.selected = None);
            }
            "telemetry-tab" => inspector(true),
            "files-tab" => inspector(false),
            "files-toggle" => {
                let target = document()
                    .query_selector(".terminal-window")
                    .unwrap()
                    .unwrap();
                let closed = target
                    .class_list()
                    .toggle("inspector-closed")
                    .unwrap_or(false);
                attribute("files-toggle", "aria-expanded", &(!closed).to_string());
                fit();
            }
            _ => {}
        }
    })?;
    bind("input", |event| {
        if let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
        {
            if target.id().starts_with("compression-") {
                update_compression();
            }
        }
    })?;
    bind("submit", |event| {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
        else {
            return;
        };
        match target.id().as_str() {
            "compression-form" => {
                event.prevent_default();
                prepare(compression());
            }
            "line-form" => {
                event.prevent_default();
                send(format!("{}\r", value("line-input")));
                input_value("line-input", "");
                terminal_focus();
            }
            _ => {}
        }
    })?;
    Ok(())
}

fn interval(milliseconds: i32, function: fn()) -> Result<(), JsValue> {
    let callback = Closure::<dyn FnMut()>::new(function);
    window().set_interval_with_callback_and_timeout_and_arguments_0(
        callback.as_ref().unchecked_ref(),
        milliseconds,
    )?;
    callback.forget();
    Ok(())
}

async fn initialize() -> Result<(), JsValue> {
    element("wasm-root").set_inner_html(include_str!("shell.html"));
    attribute("wasm-root", "data-runtime", "rust-wasm");
    attribute("wasm-root", "data-version", env!("CARGO_PKG_VERSION"));
    let data = Closure::<dyn FnMut(String)>::new(send);
    let size = Closure::<dyn FnMut(u32, u32)>::new(resize);
    terminal_create(data.as_ref().unchecked_ref(), size.as_ref().unchecked_ref())?;
    data.forget();
    size.forget();
    controls()?;
    ready(false, true);
    let meta = api("/api/meta", None).await?;
    set_text(
        "version",
        &format!(
            "{} / WASM {}",
            text(&meta["version"]).replace("devtool ", ""),
            env!("CARGO_PKG_VERSION")
        ),
    );
    html("catalog-panel", &render::catalog(&meta["schema"]));
    let existing = window()
        .local_storage()
        .ok()
        .flatten()
        .and_then(|storage| storage.get_item("devtool-terminal-session").ok().flatten());
    start_session(existing).await?;
    interval(140, output)?;
    interval(900, refresh)?;
    interval(350, refresh_telemetry)?;
    attribute("wasm-root", "data-ready", "true");
    Ok(())
}

#[wasm_bindgen(start)]
pub fn start() {
    spawn_local(async {
        if let Err(error) = initialize().await {
            connection("Fout bij opstarten", "error");
            hidden("terminal-error", false);
            set_text("terminal-error", &error_text(error.clone()));
            report(error);
        }
    });
}
