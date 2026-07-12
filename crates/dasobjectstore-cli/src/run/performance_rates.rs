use super::CliError;
use dasobjectstore_core::ids::DiskId;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub(super) struct PerformanceLiveRateCounters {
    inner: Arc<Mutex<PerformanceLiveRateState>>,
}

#[derive(Debug, Default)]
struct PerformanceLiveRateState {
    ssd_read: PerformanceMeasurementAccumulator,
    hdd_write: PerformanceMeasurementAccumulator,
}

#[derive(Clone, Copy, Debug, Default)]
struct PerformanceMeasurementAccumulator {
    bytes: u64,
    seconds: f64,
}

impl PerformanceLiveRateCounters {
    pub(super) fn add_ssd_read_interval(&self, bytes: u64, seconds: f64) -> Result<(), CliError> {
        if bytes == 0 || seconds <= 0.0 {
            return Ok(());
        }
        let mut state = self.inner.lock().map_err(|_| {
            CliError::CommandFailed("performance-test live rate lock poisoned".to_string())
        })?;
        state.ssd_read.bytes = state.ssd_read.bytes.saturating_add(bytes);
        state.ssd_read.seconds += seconds;
        Ok(())
    }

    pub(super) fn add_hdd_write_interval(
        &self,
        _disk_id: &DiskId,
        bytes: u64,
        seconds: f64,
    ) -> Result<(), CliError> {
        if seconds <= 0.0 {
            return Ok(());
        }
        let mut state = self.inner.lock().map_err(|_| {
            CliError::CommandFailed("performance-test live rate lock poisoned".to_string())
        })?;
        state.hdd_write.bytes = state.hdd_write.bytes.saturating_add(bytes);
        state.hdd_write.seconds += seconds;
        Ok(())
    }

    pub(super) fn snapshot(&self) -> Result<PerformanceLiveRateSnapshot, CliError> {
        let state = self.inner.lock().map_err(|_| {
            CliError::CommandFailed("performance-test live rate lock poisoned".to_string())
        })?;
        Ok(PerformanceLiveRateSnapshot {
            ssd_read_rate: accumulated_rate(state.ssd_read),
            hdd_write_rate: accumulated_rate(state.hdd_write),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct PerformanceLiveRateSnapshot {
    pub(super) ssd_read_rate: Option<f64>,
    pub(super) hdd_write_rate: Option<f64>,
}

fn accumulated_rate(measurement: PerformanceMeasurementAccumulator) -> Option<f64> {
    if measurement.bytes == 0 || measurement.seconds <= 0.0 {
        None
    } else {
        Some(measurement.bytes as f64 / measurement.seconds.max(0.001))
    }
}

#[cfg(test)]
mod tests {
    use super::PerformanceLiveRateCounters;
    use dasobjectstore_core::ids::DiskId;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn performance_live_rates_ignore_idle_time_between_callbacks() {
        let counters = PerformanceLiveRateCounters::default();
        let disk_id = DiskId::new("disk-a").expect("disk id");

        counters
            .add_ssd_read_interval(1_000, 2.0)
            .expect("ssd read interval");
        counters
            .add_hdd_write_interval(&disk_id, 1_000, 2.0)
            .expect("hdd write interval");
        let before_idle = counters.snapshot().expect("snapshot");
        thread::sleep(Duration::from_millis(5));
        let after_idle = counters.snapshot().expect("snapshot");

        assert_eq!(before_idle.ssd_read_rate, after_idle.ssd_read_rate);
        assert_eq!(before_idle.hdd_write_rate, after_idle.hdd_write_rate);
    }

    #[test]
    fn performance_live_rates_charge_sync_only_time_to_hdd_not_ssd_read() {
        let counters = PerformanceLiveRateCounters::default();
        let disk_id = DiskId::new("disk-a").expect("disk id");

        counters
            .add_ssd_read_interval(1_000, 1.0)
            .expect("ssd read interval");
        counters
            .add_hdd_write_interval(&disk_id, 1_000, 1.0)
            .expect("hdd write interval");
        counters
            .add_hdd_write_interval(&disk_id, 0, 3.0)
            .expect("hdd sync interval");
        let snapshot = counters.snapshot().expect("snapshot");

        assert_eq!(snapshot.ssd_read_rate, Some(1_000.0));
        assert_eq!(snapshot.hdd_write_rate, Some(250.0));
    }
}
