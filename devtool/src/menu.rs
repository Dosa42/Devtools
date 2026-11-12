use crate::{
    config::{self, KEYS},
    error::{Error, Result, io},
};
use std::{
    io::{BufRead, Write},
    path::Path,
};

fn line(input: &mut dyn BufRead, output: &mut dyn Write, prompt: &str) -> Result<String> {
    write!(output, "{prompt}").map_err(|error| io("Write menu", Path::new("stdout"), error))?;
    output
        .flush()
        .map_err(|error| io("Flush menu", Path::new("stdout"), error))?;
    let mut text = String::new();
    if input
        .read_line(&mut text)
        .map_err(|error| io("Read menu", Path::new("stdin"), error))?
        == 0
    {
        return Err(Error::Message(
            "Configuration menu input ended before save or quit".into(),
        ));
    }
    Ok(text.trim().to_owned())
}

pub fn run(path: &Path, input: &mut dyn BufRead, output: &mut dyn Write) -> Result<()> {
    let (mut settings, _) = config::stored(path, false)?;
    loop {
        let value = serde_json::to_value(&settings)?;
        writeln!(output, "\nConfiguration: {}", path.display())
            .map_err(|error| io("Write menu", path, error))?;
        for (index, key) in KEYS.iter().enumerate() {
            writeln!(
                output,
                "  {}. {} = {}",
                index + 1,
                key,
                config::get(&value, key)?
            )
            .map_err(|error| io("Write menu", path, error))?;
        }
        let choice = line(input, output, "Choose a number, s to save, q to quit: ")?;
        match choice.as_str() {
            "s" | "save" => {
                config::save(path, &settings, true)?;
                writeln!(output, "Saved {}", path.display())
                    .map_err(|error| io("Write menu", path, error))?;
                return Ok(());
            }
            "q" | "quit" => return Ok(()),
            _ => {
                let selected = choice
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| KEYS.get(index));
                if let Some(key) = selected {
                    let text = line(input, output, &format!("New value for {key}: "))?;
                    if let Err(error) = config::set(&mut settings, key, &text) {
                        writeln!(output, "{error}")
                            .map_err(|error| io("Write menu", path, error))?;
                    }
                } else {
                    writeln!(output, "Choose 1-{}, s or q.", KEYS.len())
                        .map_err(|error| io("Write menu", path, error))?;
                }
            }
        }
    }
}
