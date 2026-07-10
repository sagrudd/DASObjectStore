//! Enclosure preparation job state and polling.

use super::*;

pub(crate) fn clear_enclosure_job_monitor(state: &mut EnclosureWizardState) {
    state.job = None;
    state.job_status = None;
    state.job_polling = false;
    state.job_status_error = None;
    state.cancelling = false;
    state.cancellation = None;
    state.cancel_error = None;
    state.error = None;
}

pub(crate) fn admin_job_state_is_terminal(state: &str) -> bool {
    matches!(state, "complete" | "failed" | "cancelled")
}

pub(crate) fn admin_job_percent(job: &AdminJobSummary) -> Option<u8> {
    job.percent_complete.or_else(|| {
        (job.progress.work_units_total > 0).then(|| {
            ((job.progress.work_units_done.saturating_mul(100) / job.progress.work_units_total)
                .min(100)) as u8
        })
    })
}

pub(crate) fn admin_job_progress_text(job: &AdminJobSummary) -> String {
    if job.progress.work_bytes_total > 0 {
        return format!(
            "{} / {} byte(s)",
            job.progress.work_bytes_done, job.progress.work_bytes_total
        );
    }
    if job.progress.work_units_total > 0 {
        return format!(
            "{} / {} step(s)",
            job.progress.work_units_done, job.progress.work_units_total
        );
    }
    "Progress pending".to_string()
}

pub(crate) fn enclosure_prepare_confirmed(
    allow_format: bool,
    existing_data_acknowledged: bool,
    confirmation_phrase: &str,
) -> bool {
    allow_format
        && existing_data_acknowledged
        && confirmation_phrase.trim() == "confirm prepare das"
}

pub(crate) fn enclosure_retry_clears_job_state(state: &mut EnclosureWizardState) {
    clear_enclosure_job_monitor(state);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn enclosure_wizard_job_id(state: &EnclosureWizardState) -> Option<String> {
    state.job.as_ref().map(|job| job.accepted.job_id.clone())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn schedule_enclosure_job_status_poll(
    api_base_path: String,
    wizard_state: UseStateHandle<EnclosureWizardState>,
    job_id: String,
    delay_ms: u32,
) {
    Timeout::new(delay_ms, move || {
        let api_base_path = api_base_path.clone();
        let wizard_state = wizard_state.clone();
        let job_id = job_id.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = crate::api::get_admin_job_status(&api_base_path, &job_id).await;
            let mut should_continue = false;
            let mut next = (*wizard_state).clone();
            if enclosure_wizard_job_id(&next).as_deref() != Some(job_id.as_str()) {
                return;
            }
            next.job_polling = false;
            match result {
                Ok(status) => {
                    should_continue = !admin_job_state_is_terminal(&status.job.state);
                    next.job_status = Some(status);
                    next.job_status_error = None;
                    if should_continue {
                        next.job_polling = true;
                    }
                }
                Err(error) => {
                    next.job_status_error = Some(error.message);
                }
            }
            wizard_state.set(next);
            if should_continue {
                schedule_enclosure_job_status_poll(api_base_path, wizard_state, job_id, 2_000);
            }
        });
    })
    .forget();
}
