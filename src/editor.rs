use crate::Result;
use std::{
    io::{Read, Write},
    process::Command,
};

pub fn resolve_editor(editor: Option<&str>, visual: Option<&str>) -> String {
    editor
        .filter(|value| !value.trim().is_empty())
        .or_else(|| visual.filter(|value| !value.trim().is_empty()))
        .unwrap_or("vi")
        .to_owned()
}

/// Invoke an editor directly, with quoted arguments but no shell evaluation.
pub fn edit_prompt(editor: &str, initial: &str) -> Result<String> {
    let args = shell_words::split(editor).map_err(|_| "invalid quoting in EDITOR")?;
    let (program, args) = args.split_first().ok_or("EDITOR must not be empty")?;
    let mut file = tempfile::Builder::new()
        .prefix("clisage-prompt-")
        .suffix(".md")
        .tempfile()
        .map_err(|_| "unable to create temporary prompt file")?;
    file.write_all(initial.as_bytes())?;
    file.flush()?;
    let status = Command::new(program)
        .args(args)
        .arg(file.path())
        .status()
        .map_err(|_| "unable to start editor; check EDITOR")?;
    if !status.success() {
        return Err("editor failed or was cancelled; no request sent".into());
    }
    // Editors may atomically replace the file, so reopen its path.
    let edited = std::fs::File::open(file.path()).map_err(|_| "unable to read edited prompt")?;
    let mut prompt = String::new();
    edited
        .take(1_048_577)
        .read_to_string(&mut prompt)
        .map_err(|_| "edited prompt must contain UTF-8 text")?;
    if prompt.len() > 1_048_576 {
        return Err("edited prompt exceeds 1 MiB".into());
    }
    if prompt.trim().is_empty() {
        return Err("empty edited prompt; no request sent".into());
    }
    Ok(prompt)
}
