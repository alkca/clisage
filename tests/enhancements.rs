use clisage::{Mode, RuntimeContext, build_messages_with_context, edit_prompt, render_markdown};

#[test]
fn markdown_output_policy_preserves_pipes_and_command_suggestions() {
    use clisage::should_render_markdown as render;
    assert!(render(true, false, false, false, false, false));
    assert!(!render(false, false, false, false, false, false));
    assert!(render(false, true, false, false, false, false));
    assert!(!render(true, false, true, false, false, false));
    assert!(!render(true, true, false, true, false, false));
    assert!(!render(true, false, false, false, true, false));
    assert!(!render(true, true, false, false, false, true));
}

#[test]
fn supported_language_aliases_have_syntax_colors_and_preserve_unicode() {
    for (language, code) in [
        ("sh", "echo 'café'"),
        ("rs", "fn main() { /* café */ }"),
        ("py", "print('café')"),
        ("js", "const word = 'café';"),
        ("json", "{\"word\": \"café\"}"),
    ] {
        let rendered = render_markdown(&format!("```{language}\n{code}\n```\n"));
        assert!(rendered.contains("\x1b[38;2;"), "missing {language} colors");
        assert!(rendered.contains("café"));
    }
}

#[test]
fn context_falls_back_without_distribution_or_shell_metadata() {
    for os in ["linux", "macos"] {
        let context = RuntimeContext::new(os, "PRETTY_NAME=\"\"", "");
        let messages =
            build_messages_with_context(Mode::ShellCommand, "list", "", &context).unwrap();
        let system = messages[0]["content"].as_str().unwrap();
        assert!(system.contains(&format!("OS: {os}")));
        assert!(system.contains("Shell: unknown"));
        assert!(system.contains("POSIX"));
    }
}

#[test]
fn context_is_explicit_in_every_request_and_cannot_add_prompt_lines() {
    let context = RuntimeContext::new(
        "Linux",
        "NAME=Example\nPRETTY_NAME=\"Example Linux 42\"\n",
        "/bin/zsh",
    );
    for mode in [Mode::Prompt, Mode::Shell, Mode::ShellCommand, Mode::Explain] {
        let messages = build_messages_with_context(mode, "hello", "", &context).unwrap();
        let system = messages[0]["content"].as_str().unwrap();
        assert!(system.contains("Example Linux 42"));
        assert!(system.contains("Shell: zsh"));
        assert!(!system.contains("/bin/"));
    }
    let context = RuntimeContext::new("macos", "", "/bin/bash\ninjected");
    let messages = build_messages_with_context(Mode::Shell, "hello", "", &context).unwrap();
    let system = messages[0]["content"].as_str().unwrap();
    assert!(system.contains("macos"));
    assert!(!system.contains("injected"));
}

#[test]
fn markdown_formats_structure_and_highlights_code_without_losing_content() {
    let rendered = render_markdown(
        "# Title\n\n**Bold** and `inline` [docs](https://example.invalid).\n\n- first\n- second\n\n```rust\nfn main() { println!(\"hello\"); }\n```\n",
    );
    assert!(rendered.contains("\x1b["));
    assert!(!rendered.contains("```"));
    for text in [
        "Title",
        "Bold",
        "inline",
        "https://example.invalid",
        "first",
        "second",
        "main",
        "hello",
    ] {
        assert!(rendered.contains(text), "missing {text}");
    }
    assert!(
        rendered.contains("\x1b[38;2;"),
        "code should have syntax colors"
    );
    let unknown = render_markdown("```unknown-language\nkeep this\n```\n");
    assert!(unknown.contains("keep this"));
    let malicious = render_markdown("text\x1b]52;c;payload\x07\n");
    assert!(!malicious.contains("\x1b]52"));
    let entities = render_markdown("&#27;[31mhello&#7;\n");
    assert!(!entities.contains("\x1b[31m"));
    assert!(!entities.contains('\x07'));
}

#[cfg(unix)]
#[test]
fn editor_supports_arguments_spaces_failures_and_empty_cancellation() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let editor = dir.path().join("fake editor");
    std::fs::write(&editor, "#!/bin/sh\n[ \"$1\" = --wait ] || exit 3\n[ \"$(cat \"$2\")\" = initial ] || exit 4\nprintf '%s' \"$2\" > \"$0.path\"\nprintf 'edited prompt' > \"$2\"\n").unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        edit_prompt(&format!("'{}' --wait", editor.display()), "initial").unwrap(),
        "edited prompt"
    );
    assert!(edit_prompt("/bin/false", "initial").is_err());
    assert!(edit_prompt("/bin/true", "").is_err());
    assert!(edit_prompt("'unterminated", "").is_err());
    assert!(edit_prompt("/does/not/exist", "initial").is_err());
    let prompt_path = std::fs::read_to_string(dir.path().join("fake editor.path")).unwrap();
    assert!(
        !std::path::Path::new(&prompt_path).exists(),
        "temporary prompt should be removed"
    );
}
