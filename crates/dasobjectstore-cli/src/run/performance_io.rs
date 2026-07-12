use super::*;

pub(super) fn measure_generate_random_file_with_progress(
    path: &Path,
    size_bytes: u64,
    seed: u32,
    mut progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
    sync_policy: PerformanceCopySyncPolicy<'_>,
) -> Result<PerformanceMeasurement, CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let started = Instant::now();
    let mut file = File::create(path)?;
    let mut remaining = size_bytes;
    let mut written = 0_u64;
    let progress_step = performance_progress_step(size_bytes);
    let mut next_progress = progress_step.min(size_bytes);
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ u64::from(seed);
    while remaining > 0 {
        check_performance_cancelled()?;
        fill_pseudorandom(&mut buffer, &mut state);
        let write_len = remaining.min(buffer.len() as u64) as usize;
        file.write_all(&buffer[..write_len])?;
        remaining -= write_len as u64;
        written = written.saturating_add(write_len as u64);
        if let Some(callback) = progress.as_deref_mut() {
            if written >= next_progress || written == size_bytes {
                callback(written, started.elapsed().as_secs_f64().max(0.001))?;
                next_progress = written.saturating_add(progress_step).min(size_bytes);
            }
        }
    }
    check_performance_cancelled()?;
    match sync_policy {
        PerformanceCopySyncPolicy::SyncAll => {
            performance_sync_all(&file)?;
        }
        PerformanceCopySyncPolicy::AsyncSsdSettle(settler) => {
            settler.submit(path.to_path_buf(), file)?;
        }
    }
    if let Some(callback) = progress.as_deref_mut() {
        callback(size_bytes, started.elapsed().as_secs_f64().max(0.001))?;
    }
    Ok(PerformanceMeasurement {
        bytes: size_bytes,
        seconds: started.elapsed().as_secs_f64().max(0.001),
    })
}

pub(super) fn measure_ssd_stage_payload(
    payload: &PerformancePayload,
    destination: &Path,
    settler: &PerformanceSsdSettler,
) -> Result<PerformanceMeasurement, CliError> {
    measure_ssd_stage_payload_with_progress(payload, destination, payload.file_index, None, settler)
}

pub(super) fn measure_ssd_stage_payload_with_progress(
    payload: &PerformancePayload,
    destination: &Path,
    seed: u32,
    progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
    settler: &PerformanceSsdSettler,
) -> Result<PerformanceMeasurement, CliError> {
    measure_land_payload_with_progress_and_sync_policy(
        payload,
        destination,
        seed,
        progress,
        PerformanceCopySyncPolicy::AsyncSsdSettle(settler),
    )
}

pub(super) fn measure_land_payload_with_progress_and_sync_policy(
    payload: &PerformancePayload,
    destination: &Path,
    seed: u32,
    progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
    sync_policy: PerformanceCopySyncPolicy<'_>,
) -> Result<PerformanceMeasurement, CliError> {
    if let Some(source) = &payload.source_path {
        measure_copy_with_progress_and_sync_policy(source, destination, progress, sync_policy)
    } else {
        measure_generate_random_file_with_progress(
            destination,
            payload.size_bytes,
            seed,
            progress,
            sync_policy,
        )
    }
}

#[derive(Clone, Copy)]
pub(super) enum PerformanceCopySyncPolicy<'a> {
    SyncAll,
    AsyncSsdSettle(&'a PerformanceSsdSettler),
}

fn fill_pseudorandom(buffer: &mut [u8], state: &mut u64) {
    for chunk in buffer.chunks_mut(8) {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        let bytes = state.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}

#[cfg(test)]
pub(super) fn measure_copy_with_progress(
    source: &Path,
    destination: &Path,
    progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
) -> Result<PerformanceMeasurement, CliError> {
    measure_copy_with_progress_and_sync_policy(
        source,
        destination,
        progress,
        PerformanceCopySyncPolicy::SyncAll,
    )
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PerformanceSplitCopyProgress {
    pub(super) bytes: u64,
    pub(super) source_read_seconds: f64,
    pub(super) destination_write_seconds: f64,
    pub(super) phase: PerformanceCopyProgressPhase,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PerformanceSplitCopyMeasurement {
    pub(super) source_read: PerformanceMeasurement,
    pub(super) destination_write: PerformanceMeasurement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PerformanceCopyProgressPhase {
    Copying,
    Syncing,
}

pub(super) fn measure_copy_with_split_progress(
    source: &Path,
    destination: &Path,
    mut progress: Option<&mut dyn FnMut(PerformanceSplitCopyProgress) -> Result<(), CliError>>,
) -> Result<PerformanceSplitCopyMeasurement, CliError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut reader = File::open(source)?;
    let mut writer = File::create(destination)?;
    let mut bytes = 0_u64;
    let mut source_read_seconds = 0.0_f64;
    let mut destination_write_seconds = 0.0_f64;
    let total_bytes = source
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let progress_step = performance_progress_step(total_bytes);
    let mut next_progress = progress_step.min(total_bytes);
    let mut last_progress_emit = Instant::now();
    let mut buffer = vec![0_u8; 4 * 1024 * 1024];
    if let Some(callback) = progress.as_deref_mut() {
        callback(PerformanceSplitCopyProgress {
            bytes,
            source_read_seconds,
            destination_write_seconds,
            phase: PerformanceCopyProgressPhase::Copying,
        })?;
    }
    loop {
        check_performance_cancelled()?;
        let read_started = Instant::now();
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        source_read_seconds += read_started.elapsed().as_secs_f64();
        let write_started = Instant::now();
        writer.write_all(&buffer[..read])?;
        destination_write_seconds += write_started.elapsed().as_secs_f64();
        bytes = bytes.saturating_add(read as u64);
        if let Some(callback) = progress.as_deref_mut() {
            if bytes >= next_progress
                || bytes == total_bytes
                || last_progress_emit.elapsed() >= Duration::from_secs(1)
            {
                callback(PerformanceSplitCopyProgress {
                    bytes,
                    source_read_seconds,
                    destination_write_seconds,
                    phase: PerformanceCopyProgressPhase::Copying,
                })?;
                last_progress_emit = Instant::now();
                if bytes >= next_progress {
                    next_progress = bytes.saturating_add(progress_step).min(total_bytes);
                }
            }
        }
    }
    check_performance_cancelled()?;
    let sync_started = Instant::now();
    if let Some(callback) = progress.as_deref_mut() {
        callback(PerformanceSplitCopyProgress {
            bytes,
            source_read_seconds,
            destination_write_seconds,
            phase: PerformanceCopyProgressPhase::Syncing,
        })?;
        performance_sync_all_with_heartbeat(&writer, || {
            callback(PerformanceSplitCopyProgress {
                bytes,
                source_read_seconds,
                destination_write_seconds: destination_write_seconds
                    + sync_started.elapsed().as_secs_f64(),
                phase: PerformanceCopyProgressPhase::Syncing,
            })
        })?;
    } else {
        performance_sync_all(&writer)?;
    }
    destination_write_seconds += sync_started.elapsed().as_secs_f64();
    if let Some(callback) = progress.as_deref_mut() {
        callback(PerformanceSplitCopyProgress {
            bytes,
            source_read_seconds,
            destination_write_seconds,
            phase: PerformanceCopyProgressPhase::Syncing,
        })?;
    }
    Ok(PerformanceSplitCopyMeasurement {
        source_read: PerformanceMeasurement {
            bytes,
            seconds: source_read_seconds.max(0.001),
        },
        destination_write: PerformanceMeasurement {
            bytes,
            seconds: destination_write_seconds.max(0.001),
        },
    })
}

pub(super) fn measure_copy_with_progress_and_sync_policy(
    source: &Path,
    destination: &Path,
    mut progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
    sync_policy: PerformanceCopySyncPolicy<'_>,
) -> Result<PerformanceMeasurement, CliError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let started = Instant::now();
    let mut reader = File::open(source)?;
    let mut writer = File::create(destination)?;
    let mut bytes = 0_u64;
    let total_bytes = source
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let progress_step = performance_progress_step(total_bytes);
    let mut next_progress = progress_step.min(total_bytes);
    let mut buffer = vec![0_u8; 4 * 1024 * 1024];
    loop {
        check_performance_cancelled()?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        bytes = bytes.saturating_add(read as u64);
        if let Some(callback) = progress.as_deref_mut() {
            if bytes >= next_progress || bytes == total_bytes {
                callback(bytes, started.elapsed().as_secs_f64().max(0.001))?;
                next_progress = bytes.saturating_add(progress_step).min(total_bytes);
            }
        }
    }
    check_performance_cancelled()?;
    match sync_policy {
        PerformanceCopySyncPolicy::SyncAll => {
            performance_sync_all(&writer)?;
        }
        PerformanceCopySyncPolicy::AsyncSsdSettle(settler) => {
            settler.submit(destination.to_path_buf(), writer)?;
        }
    }
    if let Some(callback) = progress.as_deref_mut() {
        callback(bytes, started.elapsed().as_secs_f64().max(0.001))?;
    }
    Ok(PerformanceMeasurement {
        bytes,
        seconds: started.elapsed().as_secs_f64().max(0.001),
    })
}

pub(super) fn performance_sync_all(file: &File) -> io::Result<()> {
    #[cfg(test)]
    PERFORMANCE_SYNC_ALL_CALLS.with(|calls| {
        *calls.borrow_mut() += 1;
    });
    file.sync_all()
}

#[cfg(not(test))]
fn performance_sync_all_with_heartbeat(
    file: &File,
    mut heartbeat: impl FnMut() -> Result<(), CliError>,
) -> Result<(), CliError> {
    let sync_file = file.try_clone()?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(performance_sync_all(&sync_file));
    });

    loop {
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(result) => return result.map_err(CliError::from),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                heartbeat()?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(CliError::CommandFailed(
                    "performance-test sync worker stopped before reporting completion".to_string(),
                ));
            }
        }
    }
}

#[cfg(test)]
fn performance_sync_all_with_heartbeat(
    file: &File,
    mut heartbeat: impl FnMut() -> Result<(), CliError>,
) -> Result<(), CliError> {
    heartbeat()?;
    performance_sync_all(file).map_err(CliError::from)
}

#[cfg(test)]
pub(super) fn reset_performance_sync_all_calls() {
    PERFORMANCE_SYNC_ALL_CALLS.with(|calls| {
        *calls.borrow_mut() = 0;
    });
}

#[cfg(test)]
pub(super) fn performance_sync_all_calls() -> u32 {
    PERFORMANCE_SYNC_ALL_CALLS.with(|calls| *calls.borrow())
}

pub(super) fn measure_read(source: &Path) -> Result<PerformanceMeasurement, CliError> {
    measure_read_with_progress(source, None)
}

pub(super) fn measure_read_with_progress(
    source: &Path,
    mut progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
) -> Result<PerformanceMeasurement, CliError> {
    let started = Instant::now();
    let mut reader = File::open(source)?;
    let mut bytes = 0_u64;
    let total_bytes = source
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let progress_step = performance_progress_step(total_bytes);
    let mut next_progress = progress_step.min(total_bytes);
    let mut buffer = vec![0_u8; 4 * 1024 * 1024];
    loop {
        check_performance_cancelled()?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes.saturating_add(read as u64);
        if let Some(callback) = progress.as_deref_mut() {
            if bytes >= next_progress || bytes == total_bytes {
                callback(bytes, started.elapsed().as_secs_f64().max(0.001))?;
                next_progress = bytes.saturating_add(progress_step).min(total_bytes);
            }
        }
    }
    if let Some(callback) = progress.as_deref_mut() {
        callback(bytes, started.elapsed().as_secs_f64().max(0.001))?;
    }
    Ok(PerformanceMeasurement {
        bytes,
        seconds: started.elapsed().as_secs_f64().max(0.001),
    })
}

fn performance_progress_step(total_bytes: u64) -> u64 {
    const MIN_STEP: u64 = 64 * 1024 * 1024;
    const MAX_STEP: u64 = 512 * 1024 * 1024;
    if total_bytes == 0 {
        return 1;
    }
    (total_bytes / 100)
        .clamp(MIN_STEP, MAX_STEP)
        .min(total_bytes)
}
