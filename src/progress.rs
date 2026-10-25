use std::{
    io::{self, Write},
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
    time::Duration,
};

const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const CLEAR: &str = "\r\x1b[2K";

/// Owns the animation worker, stopping and clearing it before diagnostics or
/// response text can be printed. No terminal modes or cursor visibility change.
pub struct ThinkingIndicator {
    stop: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl ThinkingIndicator {
    pub fn start(
        enabled: bool,
        icon: Option<&str>,
        mut output: impl Write + Send + 'static,
    ) -> Self {
        if !enabled {
            return Self {
                stop: None,
                worker: None,
            };
        }
        let icon: String = icon.unwrap_or("").chars().filter(|c| {
            !c.is_control() && !matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        }).take(16).collect();
        let badge = if icon.is_empty() {
            String::new()
        } else {
            format!("{icon} ")
        };
        let (stop, stopped) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("clisage-progress".into())
            .spawn(move || {
                // Start below the editable shell input, preserving the prompt line.
                if output.write_all(b"\n").is_err() {
                    return;
                }
                let mut frame = 0;
                loop {
                    let message = format!("{CLEAR}{badge}Clisage: thinking... {}", FRAMES[frame]);
                    if output
                        .write_all(message.as_bytes())
                        .and_then(|_| output.flush())
                        .is_err()
                    {
                        break;
                    }
                    match stopped.recv_timeout(Duration::from_millis(100)) {
                        Err(mpsc::RecvTimeoutError::Timeout) => frame = (frame + 1) % FRAMES.len(),
                        _ => break,
                    }
                }
                let _ = output.write_all(CLEAR.as_bytes());
                let _ = output.flush();
            })
            .ok();
        Self {
            stop: worker.as_ref().map(|_| stop),
            worker,
        }
    }

    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ThinkingIndicator {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct ThinkingOutput<'a> {
    output: &'a mut dyn Write,
    indicator: &'a mut ThinkingIndicator,
    stop_on_write: bool,
}

impl<'a> ThinkingOutput<'a> {
    pub fn new(
        output: &'a mut dyn Write,
        indicator: &'a mut ThinkingIndicator,
        stop_on_write: bool,
    ) -> Self {
        Self {
            output,
            indicator,
            stop_on_write,
        }
    }
}

impl Write for ThinkingOutput<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.stop_on_write && !bytes.is_empty() {
            self.indicator.stop();
        }
        self.output.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}
