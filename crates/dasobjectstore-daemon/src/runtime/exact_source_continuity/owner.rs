//! Owner-only immutable history/CAS adapter. No ordinary object-store provider
//! or local database can satisfy this port.

use super::codec::{self, Error, Object, Request, HISTORY_LIMIT};
use std::time::{Duration, Instant};

/// Construction is private to this module. Production selection deliberately
/// supplies None: neither a public binding nor mTLS alone proves these facts.
struct Origin {
    installation: String,
    site: String,
    owner: String,
    binding_sha256: String,
    original_enrollment: Vec<u8>,
    current_binding: Vec<u8>,
}

impl Origin {
    fn validate_data(&self, now: u64) -> Result<(), Error> {
        let original = codec::binding(&self.original_enrollment)?;
        let current = codec::binding(&self.current_binding)?;
        if codec::digest(&self.current_binding) != self.binding_sha256
            || !self.matches(
                &current.installation_id,
                &current.site_uuid,
                &current.owner,
                &self.binding_sha256,
            )
            || original.installation_id != current.installation_id
            || original.site_uuid != current.site_uuid
            || original.owner != current.owner
            || original.backend_namespace_sha256 != current.backend_namespace_sha256
            || original.enrollment_original_sha256 != current.enrollment_original_sha256
            || current.state != "active"
            || codec::decimal(&current.issued_at, true)? > now
            || codec::decimal(&current.expires_at, true)? <= now
        {
            return Err(Error::Unavailable);
        }
        // These lexical checks supplement, never replace, backend reauthentication
        // of immutable enrollment and current installed binding provenance.
        Ok(())
    }
    fn genesis(&self) -> String {
        let mut bytes = b"DAS_EXACT_SOURCE_CONTINUITY_GENESIS_V1\0".to_vec();
        bytes.extend_from_slice(&self.original_enrollment);
        codec::digest(&bytes)
    }

    fn matches(&self, installation: &str, site: &str, owner: &str, binding: &str) -> bool {
        self.installation == installation
            && self.site == site
            && self.owner == owner
            && self.binding_sha256 == binding
    }

    fn key(&self, sequence: u64) -> String {
        format!(
            "exact-source-continuity/v1/{}/{}/{}/{sequence:020}",
            self.installation, self.site, self.owner
        )
    }
}

/// A complete qualified version listing, not cached latest or a public digest.
/// A production implementation must perform fresh complete ListObjectVersions,
/// GET, successor probe and independent legal-hold/policy checks inside deadline.
struct Snapshot {
    objects: Vec<Object>,
    qualified_immutable_versions: bool,
    successor_absent: bool,
}

trait ExternalRoot {
    // Local service clock, never a timestamp from AWS or request data.
    fn now(&mut self) -> Result<u64, Error> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .map_err(|_| Error::Unavailable)
    }
    // Must authenticate both retained enrollment and current purpose/application,
    // leaf generation/revocation, installed origin and independent backend policy.
    fn reauthenticate(&mut self, origin: &Origin, deadline: Instant) -> Result<(), Error>;
    fn snapshot(&mut self, origin: &Origin, deadline: Instant) -> Result<Snapshot, Error>;
    fn conditional_create(
        &mut self,
        key: &str,
        object: &Object,
        deadline: Instant,
    ) -> Result<(), Error>;
    fn readback(&mut self, key: &str, deadline: Instant) -> Result<Object, Error>;
}

struct UnavailableRoot;

impl ExternalRoot for UnavailableRoot {
    fn reauthenticate(&mut self, _: &Origin, _: Instant) -> Result<(), Error> {
        Err(Error::Unavailable)
    }
    fn snapshot(&mut self, _: &Origin, _: Instant) -> Result<Snapshot, Error> {
        Err(Error::Unavailable)
    }
    fn conditional_create(&mut self, _: &str, _: &Object, _: Instant) -> Result<(), Error> {
        Err(Error::Unavailable)
    }
    fn readback(&mut self, _: &str, _: Instant) -> Result<Object, Error> {
        Err(Error::Unavailable)
    }
}

fn bounded(deadline: Instant) -> Result<(), Error> {
    if Instant::now() >= deadline {
        return Err(Error::Unavailable);
    }
    Ok(())
}

fn history(snapshot: Snapshot, origin: &Origin, now: u64) -> Result<Vec<Object>, Error> {
    if !snapshot.qualified_immutable_versions
        || !snapshot.successor_absent
        || snapshot.objects.len() > HISTORY_LIMIT
    {
        return Err(Error::Unavailable);
    }
    let mut root = origin.genesis();
    let mut floor = 0;
    for (index, object) in snapshot.objects.iter().enumerate() {
        object.validate()?;
        let checkpoint = &object.checkpoint;
        let sequence = u64::try_from(index).map_err(|_| Error::Unavailable)? + 1;
        let created = codec::decimal(&checkpoint.created_at, true)?;
        let next_floor = codec::decimal(&checkpoint.floor, true)?;
        if checkpoint.installation_id != origin.installation
            || checkpoint.site_uuid != origin.site
            || checkpoint.owner != origin.owner
            || codec::decimal(&checkpoint.sequence, true)? != sequence
            || checkpoint.previous_root_sha256 != root
            || created < floor
            || next_floor < floor
            || next_floor > now
            || created > now
        {
            return Err(Error::Unavailable);
        }
        root = checkpoint.root()?;
        floor = next_floor;
    }
    Ok(snapshot.objects)
}

struct Window {
    issued: u64,
    expires: u64,
    last_now: u64,
}

impl Window {
    fn new(request: &Request, now: u64) -> Result<Self, Error> {
        let (issued, expires) = match request {
            Request::Cas(cas) => (&cas.issued_at, &cas.expires_at),
            Request::Read(read) => (&read.issued_at, &read.expires_at),
        };
        let issued = codec::decimal(issued, true)?;
        let expires = codec::decimal(expires, true)?;
        if !matches!(expires.checked_sub(issued), Some(1..=2)) {
            return Err(Error::Denied);
        }
        Ok(Self {
            issued,
            expires,
            last_now: now,
        })
    }

    fn observe(&mut self, now: u64, origin: &Origin) -> Result<(), Error> {
        if now == 0 || now < self.last_now {
            return Err(Error::Unavailable);
        }
        if now < self.issued || now >= self.expires {
            return Err(Error::Denied);
        }
        origin.validate_data(now)?;
        self.last_now = now;
        Ok(())
    }
}

fn current<B: ExternalRoot>(
    backend: &mut B,
    origin: &Origin,
    window: &mut Window,
    deadline: Instant,
) -> Result<u64, Error> {
    bounded(deadline)?;
    window.observe(backend.now()?, origin)?;
    backend.reauthenticate(origin, deadline)?;
    bounded(deadline)?;
    // Reauthentication may itself consume the lifetime. Fresh local time after it
    // is required, not the time supplied to initial request decoding.
    window.observe(backend.now()?, origin)?;
    bounded(deadline)?;
    Ok(window.last_now)
}

fn create_reconciled<B: ExternalRoot>(
    backend: &mut B,
    origin: &Origin,
    candidate: &Object,
    window: &mut Window,
    deadline: Instant,
) -> Result<Object, Error> {
    current(backend, origin, window, deadline)?;
    let sequence = codec::decimal(&candidate.checkpoint.sequence, true)?;
    let key = origin.key(sequence);
    match backend.conditional_create(&key, candidate, deadline) {
        Ok(()) => {}
        Err(Error::Conflict) => {
            // A real 409/412 may be an identical competing winner. Require an
            // independently authenticated complete prefix and exact occupied key.
            let now = current(backend, origin, window, deadline)?;
            let snapshot = backend.snapshot(origin, deadline)?;
            let fresh = current(backend, origin, window, deadline)?;
            let objects = history(snapshot, origin, fresh.max(now))?;
            let index = usize::try_from(sequence - 1).map_err(|_| Error::Unavailable)?;
            if objects.get(index) != Some(candidate) {
                return Err(Error::Conflict);
            }
        }
        Err(error) => return Err(error),
    }
    current(backend, origin, window, deadline)?;
    // The backend port must verify the exact retained object, unique version and
    // legal hold. Append remains consumed when this readback/ack is unknown.
    let retained = backend.readback(&key, deadline)?;
    current(backend, origin, window, deadline)?;
    if &retained != candidate {
        return Err(Error::Conflict);
    }
    retained.validate()?;
    Ok(retained)
}

fn execute<B: ExternalRoot>(
    request: Request,
    origin: Option<&Origin>,
    backend: &mut B,
    now: u64,
    deadline: Instant,
) -> Result<Object, Error> {
    bounded(deadline)?;
    let origin = origin.ok_or(Error::Unavailable)?;
    let mut window = Window::new(&request, now)?;
    let (installation, site, owner, binding) = match &request {
        Request::Cas(cas) => (
            &cas.installation_id,
            &cas.site_uuid,
            &cas.owner,
            &cas.binding_sha256,
        ),
        Request::Read(read) => (
            &read.installation_id,
            &read.site_uuid,
            &read.owner,
            &read.binding_sha256,
        ),
    };
    if !origin.matches(installation, site, owner, binding) {
        return Err(Error::Denied);
    }
    current(backend, origin, &mut window, deadline)?;
    let snapshot = backend.snapshot(origin, deadline)?;
    let now = current(backend, origin, &mut window, deadline)?;
    let objects = history(snapshot, origin, now)?;
    let selected = match request {
        Request::Read(read) => {
            let index = if read.operation == "history" {
                let sequence =
                    codec::decimal(read.sequence.as_deref().ok_or(Error::Denied)?, true)?;
                usize::try_from(sequence - 1).map_err(|_| Error::Unavailable)?
            } else {
                objects.len().checked_sub(1).ok_or(Error::Unavailable)?
            };
            objects.get(index).cloned().ok_or(Error::Unavailable)?
        }
        Request::Cas(cas) => {
            let candidate = Object {
                checkpoint: cas.checkpoint,
                operation_original_hex: cas.operation_original_hex,
            };
            let expected = codec::decimal(&cas.expected_sequence, false)?;
            let next = expected.checked_add(1).ok_or(Error::Conflict)?;
            // Exact original retry reconciles its occupied slot BEFORE comparing
            // a newer current head; advancement cannot turn it into another append.
            if let Some(occupied) = usize::try_from(expected)
                .ok()
                .and_then(|index| objects.get(index))
            {
                let prior_floor = if expected == 0 {
                    0
                } else {
                    let index = usize::try_from(expected - 1).map_err(|_| Error::Conflict)?;
                    codec::decimal(
                        &objects.get(index).ok_or(Error::Conflict)?.checkpoint.floor,
                        true,
                    )?
                };
                if codec::decimal(&cas.expected_floor, false)? != prior_floor
                    || occupied != &candidate
                    || occupied.checkpoint.previous_root_sha256 != cas.expected_root_sha256
                    || codec::decimal(&occupied.checkpoint.created_at, true)?
                        < codec::decimal(&cas.expected_floor, false)?
                {
                    return Err(Error::Conflict);
                }
                occupied.clone()
            } else {
                let length = u64::try_from(objects.len()).map_err(|_| Error::Unavailable)?;
                let (root, floor) = objects.last().map_or_else(
                    || Ok::<(String, u64), Error>((origin.genesis(), 0)),
                    |object| {
                        Ok((
                            object.checkpoint.root()?,
                            codec::decimal(&object.checkpoint.floor, true)?,
                        ))
                    },
                )?;
                if objects.iter().any(|object| {
                    object.checkpoint.operation_id == candidate.checkpoint.operation_id
                }) || length != expected
                    || objects.len() >= HISTORY_LIMIT
                    || root != cas.expected_root_sha256
                    || floor != codec::decimal(&cas.expected_floor, false)?
                    || codec::decimal(&candidate.checkpoint.sequence, true)? != next
                    || candidate.checkpoint.previous_root_sha256 != root
                    || codec::decimal(&candidate.checkpoint.created_at, true)? < floor
                    || codec::decimal(&candidate.checkpoint.floor, true)? > now
                {
                    return Err(Error::Conflict);
                }
                create_reconciled(backend, origin, &candidate, &mut window, deadline)?
            }
        }
    };
    current(backend, origin, &mut window, deadline)?;
    Ok(selected)
}

pub(super) fn production_request(bytes: &[u8], now: u64) -> Result<(), Error> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let request = codec::decode(bytes, now)?;
    // No installed binding, AWS namespace, independent policy or native origin
    // is currently selected. No externally supplied data can change this None.
    execute(request, None, &mut UnavailableRoot, now, deadline).map(|_| ())
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;

    pub(in crate::runtime::exact_source_continuity) struct Scenario {
        pub current_binding: Vec<u8>,
        pub clocks: Vec<u64>,
        pub fail_readback: bool,
        pub competing_winner: Option<Object>,
        pub qualified: bool,
        pub authenticated: bool,
    }

    impl Scenario {
        pub(in crate::runtime::exact_source_continuity) fn ordinary(
            current_binding: Vec<u8>,
            now: u64,
            fail_readback: bool,
        ) -> Self {
            Self {
                current_binding,
                clocks: vec![now],
                fail_readback,
                competing_winner: None,
                qualified: true,
                authenticated: true,
            }
        }
    }

    pub(in crate::runtime::exact_source_continuity) fn exercise(
        request: Request,
        enrollment: Vec<u8>,
        current_binding: Vec<u8>,
        binding: String,
        objects: Vec<Object>,
        now: u64,
        fail_readback: bool,
    ) -> (Result<Object, Error>, Vec<Object>) {
        exercise_scenario(
            request,
            enrollment,
            binding,
            objects,
            now,
            Scenario::ordinary(current_binding, now, fail_readback),
        )
    }

    pub(in crate::runtime::exact_source_continuity) fn exercise_scenario(
        request: Request,
        enrollment: Vec<u8>,
        binding: String,
        objects: Vec<Object>,
        now: u64,
        scenario: Scenario,
    ) -> (Result<Object, Error>, Vec<Object>) {
        struct Memory {
            objects: Vec<Object>,
            clocks: std::collections::VecDeque<u64>,
            clock: u64,
            fail_readback: bool,
            winner: Option<Object>,
            qualified: bool,
            authenticated: bool,
        }
        impl ExternalRoot for Memory {
            fn now(&mut self) -> Result<u64, Error> {
                if let Some(now) = self.clocks.pop_front() {
                    self.clock = now;
                }
                Ok(self.clock)
            }
            fn reauthenticate(&mut self, _: &Origin, _: Instant) -> Result<(), Error> {
                if self.authenticated {
                    Ok(())
                } else {
                    Err(Error::Unavailable)
                }
            }
            fn snapshot(&mut self, _: &Origin, _: Instant) -> Result<Snapshot, Error> {
                Ok(Snapshot {
                    objects: self.objects.clone(),
                    qualified_immutable_versions: self.qualified,
                    successor_absent: true,
                })
            }
            fn conditional_create(
                &mut self,
                _: &str,
                object: &Object,
                _: Instant,
            ) -> Result<(), Error> {
                if let Some(winner) = self.winner.take() {
                    self.objects.push(winner);
                    return Err(Error::Conflict);
                }
                let sequence = codec::decimal(&object.checkpoint.sequence, true)?;
                if sequence <= u64::try_from(self.objects.len()).map_err(|_| Error::Unavailable)? {
                    return Err(Error::Conflict);
                }
                self.objects.push(object.clone());
                Ok(())
            }
            fn readback(&mut self, key: &str, _: Instant) -> Result<Object, Error> {
                if self.fail_readback {
                    return Err(Error::Unavailable);
                }
                let sequence = key
                    .rsplit('/')
                    .next()
                    .ok_or(Error::Denied)?
                    .parse::<usize>()
                    .map_err(|_| Error::Denied)?;
                self.objects
                    .get(sequence.checked_sub(1).ok_or(Error::Denied)?)
                    .cloned()
                    .ok_or(Error::Unavailable)
            }
        }
        let (installation, site, owner) = match &request {
            Request::Cas(value) => (
                value.installation_id.clone(),
                value.site_uuid.clone(),
                value.owner.clone(),
            ),
            Request::Read(value) => (
                value.installation_id.clone(),
                value.site_uuid.clone(),
                value.owner.clone(),
            ),
        };
        let origin = Origin {
            installation,
            site,
            owner,
            binding_sha256: binding,
            original_enrollment: enrollment,
            current_binding: scenario.current_binding,
        };
        let mut backend = Memory {
            objects,
            clocks: scenario.clocks.into(),
            clock: now,
            fail_readback: scenario.fail_readback,
            winner: scenario.competing_winner,
            qualified: scenario.qualified,
            authenticated: scenario.authenticated,
        };
        let result = execute(
            request,
            Some(&origin),
            &mut backend,
            now,
            Instant::now() + Duration::from_secs(2),
        );
        (result, backend.objects)
    }
}
