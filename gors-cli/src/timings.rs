use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct TimingCollector {
    inner: Arc<TimingCollectorInner>,
}

struct TimingCollectorInner {
    started: Instant,
    profile_stderr: bool,
    phases: Mutex<Vec<PhaseTiming>>,
    cache_events: Mutex<Vec<CacheEvent>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhaseTiming {
    name: &'static str,
    duration_ms: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheEvent {
    layer: &'static str,
    hit: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TimingReport<'a> {
    version: u32,
    command: &'a str,
    total_ms: f64,
    phases: &'a [PhaseTiming],
    cache_events: &'a [CacheEvent],
}

impl TimingCollector {
    pub fn new() -> Self {
        let profile_stderr = std::env::var("GORS_PROFILE")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        Self {
            inner: Arc::new(TimingCollectorInner {
                started: Instant::now(),
                profile_stderr,
                phases: Mutex::new(Vec::new()),
                cache_events: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn phase(&self, name: &'static str) -> PhaseTimer {
        PhaseTimer {
            name,
            started: Instant::now(),
            collector: self.clone(),
        }
    }

    pub fn cache_event(&self, layer: &'static str, hit: bool) {
        if let Ok(mut events) = self.inner.cache_events.lock() {
            events.push(CacheEvent { layer, hit });
        }
    }

    pub fn write_json(
        &self,
        path: Option<&Path>,
        command: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(path) = path else {
            return Ok(());
        };

        let phases = self
            .inner
            .phases
            .lock()
            .map_err(|_| "timing phase storage is unavailable")?
            .clone();
        let cache_events = self
            .inner
            .cache_events
            .lock()
            .map_err(|_| "timing cache-event storage is unavailable")?
            .clone();
        let report = TimingReport {
            version: 2,
            command,
            total_ms: duration_ms(self.inner.started.elapsed()),
            phases: &phases,
            cache_events: &cache_events,
        };

        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(temp.as_file_mut(), &report)?;
        use std::io::Write as _;
        temp.as_file_mut().write_all(b"\n")?;
        temp.as_file_mut().sync_all()?;
        temp.persist(path).map_err(|error| error.error)?;
        Ok(())
    }
}

pub struct PhaseTimer {
    name: &'static str,
    started: Instant,
    collector: TimingCollector,
}

impl Drop for PhaseTimer {
    fn drop(&mut self) {
        let duration_ms = duration_ms(self.started.elapsed());
        if self.collector.inner.profile_stderr {
            eprintln!("[gors-profile] {}: {:.2}ms", self.name, duration_ms);
        }
        if let Ok(mut phases) = self.collector.inner.phases.lock() {
            phases.push(PhaseTiming {
                name: self.name,
                duration_ms,
            });
        }
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn writes_machine_readable_timing_report() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nested").join("timings.json");
        let collector = TimingCollector::new();
        {
            let _phase = collector.phase("cli.test");
        }
        collector.cache_event("compiler", true);
        collector
            .write_json(Some(&path), "build")
            .expect("timing report");

        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(value.get("version").unwrap(), 2);
        assert_eq!(value.get("command").unwrap(), "build");
        assert!(value.get("jobs").is_none());
        let first_phase = value
            .get("phases")
            .and_then(serde_json::Value::as_array)
            .and_then(|phases| phases.first())
            .unwrap();
        assert_eq!(first_phase.get("name").unwrap(), "cli.test");
        let first_cache_event = value
            .get("cacheEvents")
            .and_then(serde_json::Value::as_array)
            .and_then(|events| events.first())
            .unwrap();
        assert_eq!(first_cache_event.get("layer").unwrap(), "compiler");
        assert_eq!(first_cache_event.get("hit").unwrap(), true);
    }
}
