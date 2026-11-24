//! `adb logcat` output, written straight to liblog under the tag `ursa`: warnings, plus the
//! Ursa crates' own events at info. Each event is formatted into its own buffer and dropped
//! after writing (a pooled writer keeps every buffer it was ever handed; Street Cryptid found
//! 74 MiB held that way).

use std::ffi::{c_char, c_int};
use std::io::Write;

use tracing::Level;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::MakeWriter;

#[link(name = "log")]
unsafe extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

/// logd truncates at 4096 bytes.
const MAX_LINE: usize = 4000;
const TAG: &[u8] = b"ursa\0";

pub(crate) fn layer<S>() -> impl Layer<S> + Send + Sync
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_writer(Logcat)
        .with_filter(
            Targets::new()
                .with_target("ursa_ffi", Level::INFO)
                .with_target("ursa_client", Level::INFO)
                .with_target("ursa_proto", Level::INFO)
                .with_default(Level::WARN),
        )
}

struct Logcat;

struct Line {
    priority: c_int,
    text: Vec<u8>,
}

impl<'a> MakeWriter<'a> for Logcat {
    type Writer = Line;

    fn make_writer(&'a self) -> Line {
        Line {
            priority: 4,
            text: Vec::new(),
        }
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Line {
        let priority = match *meta.level() {
            Level::ERROR => 6,
            Level::WARN => 5,
            Level::INFO => 4,
            Level::DEBUG => 3,
            Level::TRACE => 2,
        };
        Line {
            priority,
            text: Vec::new(),
        }
    }
}

impl Write for Line {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.text.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        for chunk in self.text.chunks(MAX_LINE) {
            let mut line: Vec<u8> = chunk.iter().copied().filter(|b| *b != 0).collect();
            line.push(0);
            // SAFETY: both strings are NUL-terminated and outlive the call.
            unsafe {
                __android_log_write(self.priority, TAG.as_ptr().cast(), line.as_ptr().cast());
            }
        }
    }
}
