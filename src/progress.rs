use crate::matcher::ProgressCallback;
use log::info;
use std::sync::{Arc, Mutex};

pub fn logging_progress_callback(
    activity: &'static str,
    unit_label: &'static str,
    total_hint: usize,
) -> ProgressCallback {
    let mut last_percent: Option<usize> = None;
    Arc::new(Mutex::new(move |completed: usize, total: usize| {
        let total_units = if total == 0 { total_hint.max(1) } else { total };
        let display_total = if total == 0 { total_hint } else { total };
        let done_units = if display_total == 0 {
            completed
        } else {
            completed.min(display_total)
        };

        let percent = if total_units == 0 {
            100
        } else {
            ((done_units.min(total_units) as f64 / total_units as f64) * 100.0)
                .round()
                .clamp(0.0, 100.0) as usize
        };

        let should_log = match last_percent {
            Some(prev) => percent >= prev.saturating_add(5) || (percent == 100 && percent != prev),
            None => true,
        };

        if should_log {
            let display_total_value = if display_total == 0 {
                total_hint.max(1)
            } else {
                display_total
            };
            info!(
                "{} progress: {}% ({} / {} {})",
                activity, percent, done_units, display_total_value, unit_label
            );
            last_percent = Some(percent);
        }
    }))
}
