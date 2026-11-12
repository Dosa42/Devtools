use crate::{canonical, cli::Cli, config::Settings};
use clap::{Command, CommandFactory};
use serde_json::{Value, json};

pub fn settings() -> Value {
    serde_json::to_value(schemars::schema_for!(Settings)).expect("Settings schema is serializable")
}

fn command_schema(command: &Command) -> Value {
    let arguments: Vec<Value> = command.get_arguments().map(|argument| {
        let values = argument.get_value_parser().possible_values()
            .map(|values| values.map(|value| value.get_name().to_owned()).collect::<Vec<_>>());
        json!({
            "id": argument.get_id().as_str(),
            "short": argument.get_short().map(|value| value.to_string()),
            "long": argument.get_long(),
            "help": argument.get_help().map(|help| help.to_string()),
            "required": argument.is_required_set(),
            "global": argument.is_global_set(),
            "positional": argument.is_positional(),
            "action": format!("{:?}", argument.get_action()),
            "arity": argument.get_num_args().map(|value| value.to_string()),
            "possible_values": values,
            "conflicts": command.get_arg_conflicts_with(argument).iter().map(|arg| arg.get_id().as_str()).collect::<Vec<_>>()
        })
    }).collect();
    json!({
        "name": command.get_name(),
        "about": command.get_about().map(|about| about.to_string()),
        "arguments": arguments,
        "commands": command.get_subcommands().filter(|command| command.get_name() != "help").map(command_schema).collect::<Vec<_>>()
    })
}

pub fn cli() -> Value {
    let mut command = Cli::command();
    command.build();
    json!({
        "schema_version": 1,
        "version": env!("CARGO_PKG_VERSION"),
        "cli": command_schema(&command),
        "directory_mode": {
            "flag": "-D",
            "path_flag": "--path",
            "default_parent": "current working directory",
            "name_template": "YYYYMMDDTHHMMSSZ_run-nr.NNN",
            "timestamp_timezone": "UTC",
            "initial_run": 1,
            "run_minimum_digits": 3,
            "counter_scope": "per user, shared across parent paths",
            "naming_configurable": false
        },
        "canonical_tree_mode": {
            "flag": "--canonical-tree",
            "path_flag": "--path",
            "default_target": "latest successfully created main directory",
            "explicit_path_is": "tree target itself",
            "runner": "embedded shell script executed with sh",
            "template": serde_json::from_str::<Value>(canonical::TEMPLATE_JSON).expect("Embedded template is valid JSON")
        }
    })
}
