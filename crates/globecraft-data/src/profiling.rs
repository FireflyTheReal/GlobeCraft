//! Opt-in bounded timing capture. Callers never write profile files.
use std::{
    io::{BufWriter, Write},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const PROFILE_QUEUE_EVENTS: usize = 8192;
const PROFILE_FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const PROFILE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// A UI pass this slow is treated as a visible freeze worth a trace.
const SLOW_UI_PASS_THRESHOLD: Duration = Duration::from_millis(50);
/// Sections shorter than this are folded into the pass total to keep traces small.
const SLOW_UI_SECTION_FLOOR: Duration = Duration::from_millis(5);
/// Upper bound on section events for one slow pass. The app has fewer sections;
/// the cap keeps a future coarse pass from flooding the bounded recorder.
const MAX_TRACE_SECTIONS: usize = 24;
static LOGGER: OnceLock<Option<Logger>> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);
struct Logger {
    sender: mpsc::SyncSender<Message>,
}
enum Message {
    Event(Event),
    Flush(mpsc::SyncSender<()>),
}
struct Event {
    operation: &'static str,
    unix_us: u128,
    elapsed_us: u128,
    values: Values,
}
#[derive(Clone, Copy, Default)]
pub struct Values {
    pub width: usize,
    pub height: usize,
    pub inflight: usize,
    pub resident_bytes: usize,
    pub count: usize,
}
pub fn init() {
    LOGGER.get_or_init(|| {
        let path = std::env::var_os("GLOBECRAFT_PROFILE")?;
        let (sender, events) = mpsc::sync_channel(PROFILE_QUEUE_EVENTS);
        let result = std::thread::Builder::new()
            .name("globecraft-profile".into())
            .spawn(move || {
                let result = (|| -> std::io::Result<()> {
                    // Never truncate an existing report accidentally.
                    let file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(path)?;
                    write_events(BufWriter::new(file), events)
                })();
                if let Err(error) = result {
                    eprintln!("Profile capture failed: {error}");
                }
            });
        match result {
            Ok(_) => Some(Logger { sender }),
            Err(error) => {
                eprintln!("Profile worker failed: {error}");
                None
            }
        }
    });
}
fn write_events(mut writer: impl Write, events: mpsc::Receiver<Message>) -> std::io::Result<()> {
    writeln!(
        writer,
        "unix_us,operation,elapsed_us,width,height,inflight,resident_bytes,count,dropped_events"
    )?;
    loop {
        match events.recv_timeout(PROFILE_FLUSH_INTERVAL) {
            Ok(Message::Event(event)) => {
                let v = event.values;
                writeln!(
                    writer,
                    "{},{},{},{},{},{},{},{},{}",
                    event.unix_us,
                    event.operation,
                    event.elapsed_us,
                    v.width,
                    v.height,
                    v.inflight,
                    v.resident_bytes,
                    v.count,
                    DROPPED.load(Ordering::Relaxed)
                )?;
            }
            Ok(Message::Flush(done)) => {
                writer.flush()?;
                let _ = done.send(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => writer.flush()?,
            Err(mpsc::RecvTimeoutError::Disconnected) => return writer.flush(),
        }
    }
}
pub fn flush() {
    let Some(Some(logger)) = LOGGER.get() else {
        return;
    };
    let (done, result) = mpsc::sync_channel(1);
    if logger.sender.try_send(Message::Flush(done)).is_ok() {
        let _ = result.recv_timeout(PROFILE_SHUTDOWN_TIMEOUT);
    }
}
pub struct Span {
    operation: &'static str,
    start: Instant,
    unix_us: u128,
    pub values: Values,
}
impl Span {
    pub fn new(operation: &'static str) -> Self {
        Self {
            operation,
            start: Instant::now(),
            unix_us: unix_micros(),
            values: Values::default(),
        }
    }
    pub fn elapsed(operation: &'static str, start: Instant, values: Values) {
        let span = Self {
            operation,
            start,
            values,
            unix_us: unix_micros(),
        };
        drop(span);
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        emit(Event {
            operation: self.operation,
            unix_us: self.unix_us,
            elapsed_us: self.start.elapsed().as_micros(),
            values: self.values,
        });
    }
}
/// One checkpoint in a UI pass. A section is only named, not instrumented,
/// until the pass turns out to be slow.
#[derive(Clone, Copy)]
struct TraceSection {
    operation: &'static str,
    elapsed: Duration,
}
/// Records a whole UI pass only when it exceeds [`SLOW_UI_PASS_THRESHOLD`].
///
/// `checkpoint` closes the previous section and starts the next. A fast pass
/// emits nothing; a slow pass emits one summary event followed by at most
/// [`MAX_TRACE_SECTIONS`] section events whose duration is at least
/// [`SLOW_UI_SECTION_FLOOR`]. All events share the same `unix_us`, so a report
/// can group one pass together. Normal builds compile this out entirely.
pub struct PassWatchdog {
    operation: &'static str,
    start: Instant,
    unix_us: u128,
    current: Option<(&'static str, Instant)>,
    sections: [Option<TraceSection>; MAX_TRACE_SECTIONS],
    section_count: usize,
}
impl PassWatchdog {
    pub fn new(operation: &'static str) -> Self {
        Self {
            operation,
            start: Instant::now(),
            unix_us: unix_micros(),
            current: None,
            sections: [None; MAX_TRACE_SECTIONS],
            section_count: 0,
        }
    }
    /// Ends the previous section and starts `operation`.
    pub fn checkpoint(&mut self, operation: &'static str) {
        self.close_section();
        self.current = Some((operation, Instant::now()));
    }
    /// Ends the pass and records a trace if it was slow.
    pub fn finish(mut self, values: Values) {
        self.close_section();
        let total = self.start.elapsed();
        if total < SLOW_UI_PASS_THRESHOLD {
            return;
        }
        trace_events(
            self.operation,
            self.unix_us,
            total,
            &self.sections[..self.section_count],
            values,
            emit,
        );
    }
    fn close_section(&mut self) {
        let Some((operation, start)) = self.current.take() else {
            return;
        };
        if self.section_count < MAX_TRACE_SECTIONS {
            self.sections[self.section_count] = Some(TraceSection {
                operation,
                elapsed: start.elapsed(),
            });
            self.section_count += 1;
        }
    }
}
fn trace_events(
    operation: &'static str,
    unix_us: u128,
    total: Duration,
    sections: &[Option<TraceSection>],
    values: Values,
    mut emit: impl FnMut(Event),
) {
    emit(Event {
        operation,
        unix_us,
        elapsed_us: total.as_micros(),
        values,
    });
    for section in sections
        .iter()
        .flatten()
        .filter(|section| section.elapsed >= SLOW_UI_SECTION_FLOOR)
    {
        emit(Event {
            operation: section.operation,
            unix_us,
            elapsed_us: section.elapsed.as_micros(),
            values,
        });
    }
}
fn emit(event: Event) {
    let Some(Some(logger)) = LOGGER.get() else {
        return;
    };
    if logger.sender.try_send(Message::Event(event)).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}
fn unix_micros() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_records_dimensions_duration_and_flushes() {
        let (tx, rx) = mpsc::sync_channel(4);
        tx.send(Message::Event(Event {
            operation: "ui.update",
            unix_us: 10,
            elapsed_us: 20,
            values: Values {
                width: 30,
                height: 40,
                ..Values::default()
            },
        }))
        .unwrap();
        drop(tx);
        let mut report = Vec::new();
        write_events(&mut report, rx).unwrap();
        let text = String::from_utf8(report).unwrap();
        assert!(text.contains("10,ui.update,20,30,40,0,0,0,"));
    }
    #[test]
    fn a_slow_trace_keeps_the_summary_and_sections_above_the_floor() {
        let sections = [
            Some(TraceSection {
                operation: "ui.slow.header",
                elapsed: Duration::from_millis(4),
            }),
            Some(TraceSection {
                operation: "ui.slow.globe",
                elapsed: Duration::from_millis(20),
            }),
            Some(TraceSection {
                operation: "ui.slow.panes",
                elapsed: Duration::from_millis(5),
            }),
        ];
        let mut events = Vec::new();
        trace_events(
            "ui.slow",
            42,
            Duration::from_millis(60),
            &sections,
            Values::default(),
            |event| events.push(event),
        );
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].operation, "ui.slow");
        assert_eq!(events[0].unix_us, 42);
        assert_eq!(events[0].elapsed_us, 60_000);
        assert_eq!(events[1].operation, "ui.slow.globe");
        assert_eq!(events[1].elapsed_us, 20_000);
        assert_eq!(events[2].operation, "ui.slow.panes");
    }
    #[test]
    fn checkpoints_close_the_previous_section_and_keep_one_open() {
        let mut watchdog = PassWatchdog::new("ui.slow");
        watchdog.checkpoint("ui.slow.header");
        watchdog.checkpoint("ui.slow.globe");
        assert_eq!(watchdog.section_count, 1);
        assert_eq!(watchdog.current.unwrap().0, "ui.slow.globe");
        watchdog.close_section();
        assert_eq!(watchdog.section_count, 2);
        assert!(watchdog.current.is_none());
    }
}
