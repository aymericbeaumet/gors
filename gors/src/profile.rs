use std::borrow::Cow;
use std::time::Instant;

#[derive(Clone)]
pub struct ProfileTimer {
    label: Cow<'static, str>,
    start: Option<Instant>,
}

impl ProfileTimer {
    pub fn start(label: impl Into<Cow<'static, str>>) -> Self {
        let enabled = std::env::var("GORS_PROFILE")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        Self {
            label: label.into(),
            start: enabled.then(Instant::now),
        }
    }
}

impl Drop for ProfileTimer {
    fn drop(&mut self) {
        let Some(start) = self.start else {
            return;
        };
        eprintln!(
            "[gors-profile] {}: {:.2}ms",
            self.label,
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
