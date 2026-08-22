use super::*;

// ---------------------------------------------------------------------------------------------
// The in-memory backend
// ---------------------------------------------------------------------------------------------

pub(super) struct MemSession {
    doc: HeadDoc,
    retention: JournalRetention,
    pub(super) direct_children: u32,
    pub(super) descendants: u32,
    live_sandboxes: u32,
    fence: u64,
    last_seq: u64,
    owner: Option<String>,
    lease_expires_ms: u64,
    records: std::collections::BTreeMap<u64, (u64, Record)>,
}

/// The reference store: exact semantics, zero durability, zero dependencies.
#[derive(Default)]
pub struct MemoryStore {
    pub(super) sessions: std::sync::Mutex<HashMap<String, MemSession>>,
    pub(super) tenant_storage: std::sync::Mutex<HashMap<String, u64>>,
    /// `(metered journal bytes, retained session identities)` per tenant.
    pub(super) tenant_retention: std::sync::Mutex<HashMap<String, (u64, u64)>>,
    child_links:
        std::sync::Mutex<HashMap<String, std::collections::BTreeMap<String, SessionSummary>>>,
    sandboxes:
        std::sync::Mutex<HashMap<String, std::collections::BTreeMap<String, SandboxInventoryDoc>>>,
    deletions: std::sync::Mutex<HashMap<String, DeletionStatusDoc>>,
}

#[async_trait::async_trait]
impl JournalStore for MemoryStore {
    async fn create(&self, decision: &CreateDecision<'_>) -> Result<()> {
        let &CreateDecision {
            session_id,
            doc,
            first,
            now_ms,
            tenant_storage_limit,
            retention,
            retention_limits,
        } = decision;
        validate_ancestor_path(doc)?;
        validate_config_doc(doc)?;
        validate_decision(session_id, &[(1, first.clone())], doc)?;
        if retention != initial_retention(first, retention_limits.session_bytes)? {
            return Err(BrainError::Journal(
                "create journal retention projection does not match the canonical charge".into(),
            ));
        }
        if doc.session_storage_bytes != 0 || doc.storage_reserved_bytes != 0 {
            return Err(BrainError::Invalid(
                "new sessions must start with zero public session storage".into(),
            ));
        }
        if doc.parent_id.is_some() && doc.tenant_metered_storage_bytes != 0 {
            return Err(BrainError::Invalid(
                "child sessions cannot reserve root-owned bundle storage".into(),
            ));
        }
        let mut map = self.sessions.lock().expect("memory journal");
        if map.contains_key(session_id) {
            return Err(BrainError::Invalid(format!(
                "session {session_id} already exists"
            )));
        }
        let mut tenant_storage = self
            .tenant_storage
            .lock()
            .expect("memory tenant storage meter");
        let used = tenant_storage.get(&doc.tenant_id).copied().unwrap_or(0);
        let next_tenant_storage = used
            .checked_add(doc.tenant_metered_storage_bytes)
            .ok_or_else(|| BrainError::Journal("tenant storage meter overflowed".into()))?;
        if next_tenant_storage > tenant_storage_limit {
            return Err(BrainError::TenantStorageQuotaExceeded {
                requested: doc.tenant_metered_storage_bytes,
                limit: tenant_storage_limit,
            });
        }
        let mut tenant_retention = self
            .tenant_retention
            .lock()
            .expect("memory tenant retention meter");
        let (used_journal_bytes, retained_sessions) = tenant_retention
            .get(&doc.tenant_id)
            .copied()
            .unwrap_or((0, 0));
        let next_journal_bytes = used_journal_bytes
            .checked_add(retention.metered_bytes)
            .ok_or_else(|| BrainError::Journal("tenant journal meter overflowed".into()))?;
        if next_journal_bytes > retention_limits.tenant_bytes {
            return Err(BrainError::TenantJournalQuotaExceeded {
                requested: retention.metered_bytes,
                limit: retention_limits.tenant_bytes,
            });
        }
        let next_retained_sessions = retained_sessions.checked_add(1).ok_or_else(|| {
            BrainError::Journal("tenant retained-session meter overflowed".into())
        })?;
        if next_retained_sessions > retention_limits.tenant_sessions {
            return Err(BrainError::TenantRetainedSessionQuotaExceeded {
                limit: retention_limits.tenant_sessions,
            });
        }
        if let Some(parent_id) = &doc.parent_id {
            let parent_doc = &map
                .get(parent_id)
                .ok_or_else(|| BrainError::NoSuchSession(parent_id.clone()))?
                .doc;
            let mut expected_ancestors = parent_doc.ancestor_ids.clone();
            expected_ancestors.push(parent_id.clone());
            if doc.ancestor_ids != expected_ancestors {
                return Err(BrainError::Invalid(
                    "child ancestor path does not extend its direct parent".into(),
                ));
            }
            for ancestor_id in &doc.ancestor_ids {
                let ancestor = map
                    .get(ancestor_id)
                    .ok_or_else(|| BrainError::NoSuchSession(ancestor_id.clone()))?;
                if ancestor.doc.root_id != doc.root_id || !child_admission_open(&ancestor.doc) {
                    return Err(BrainError::Invalid(
                        "child admission is closed by an ancestor fence".into(),
                    ));
                }
            }
            let parent = map
                .get(parent_id)
                .ok_or_else(|| BrainError::NoSuchSession(parent_id.clone()))?;
            let root = map
                .get(&doc.root_id)
                .ok_or_else(|| BrainError::NoSuchSession(doc.root_id.clone()))?;
            if !child_admission_open(&parent.doc)
                || !child_admission_open(&root.doc)
                || parent.doc.root_id != doc.root_id
                || doc.depth != parent.doc.depth.saturating_add(1)
                || parent.doc.depth >= root.doc.prefix.max_child_depth
            {
                return Err(BrainError::Invalid(
                    "child admission is closed or its rooted scope is stale".into(),
                ));
            }
            if parent.direct_children >= root.doc.prefix.max_direct_children
                || root.descendants >= root.doc.prefix.max_descendants
            {
                return Err(BrainError::Overloaded);
            }
            let parent_direct_children = parent.direct_children;
            let root_descendants = root.descendants;
            if parent_id == &doc.root_id {
                let root = map.get_mut(parent_id).expect("root checked above");
                root.direct_children = root.direct_children.saturating_add(1);
                root.descendants = root.descendants.saturating_add(1);
            } else {
                map.get_mut(parent_id)
                    .expect("parent checked above")
                    .direct_children = parent_direct_children.saturating_add(1);
                map.get_mut(&doc.root_id)
                    .expect("root checked above")
                    .descendants = root_descendants.saturating_add(1);
            }
        }
        let mut records = std::collections::BTreeMap::new();
        records.insert(1, (now_ms, first.clone()));
        map.insert(
            session_id.to_string(),
            MemSession {
                doc: doc.clone(),
                retention,
                direct_children: 0,
                descendants: 0,
                live_sandboxes: 0,
                fence: 0,
                last_seq: 1,
                owner: None,
                lease_expires_ms: 0,
                records,
            },
        );
        tenant_storage.insert(doc.tenant_id.clone(), next_tenant_storage);
        tenant_retention.insert(
            doc.tenant_id.clone(),
            (next_journal_bytes, next_retained_sessions),
        );
        drop(tenant_retention);
        drop(tenant_storage);
        drop(map);
        if let Some(parent_id) = &doc.parent_id {
            self.child_links
                .lock()
                .expect("memory child links")
                .entry(parent_id.clone())
                .or_default()
                .insert(
                    session_id.to_owned(),
                    SessionSummary::from_head(session_id, doc),
                );
        }
        Ok(())
    }

    async fn claim(&self, session_id: &str, owner: &str, now_ms: u64) -> Result<Head> {
        let mut map = self.sessions.lock().expect("memory journal");
        let s = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        let claimable = match &s.owner {
            None => true,
            Some(o) if o == owner => true,
            Some(_) => s.lease_expires_ms < now_ms.saturating_sub(STEAL_GRACE_MS),
        };
        if !claimable {
            return Err(BrainError::Fenced);
        }
        s.owner = Some(owner.to_string());
        s.lease_expires_ms = now_ms + LEASE_MS;
        s.fence += 1;
        Ok(Head {
            session_id: session_id.to_string(),
            doc: s.doc.clone(),
            fence: s.fence,
            last_seq: s.last_seq,
            retention: s.retention,
        })
    }

    async fn fence_end(
        &self,
        session_id: &str,
        now_ms: u64,
        retention_limits: JournalRetentionLimits,
    ) -> Result<EndFence> {
        let mut map = self.sessions.lock().expect("memory journal");
        let current = map
            .get(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        let head = Head {
            session_id: session_id.to_owned(),
            doc: current.doc.clone(),
            fence: current.fence,
            last_seq: current.last_seq,
            retention: current.retention,
        };
        let Some((doc, sequence, record)) = project_end_fence(&head, now_ms)? else {
            return Ok(EndFence {
                head,
                newly_fenced: false,
            });
        };
        let next_fence = head
            .fence
            .checked_add(1)
            .ok_or_else(|| BrainError::Journal("journal fence exhausted".into()))?;
        let next_retention = project_retention(
            head.retention,
            &[(sequence, record.clone())],
            retention_limits.session_bytes,
        )?;
        let delta = retention_delta(head.retention, next_retention)?;
        let mut tenant_retention = self
            .tenant_retention
            .lock()
            .expect("memory tenant retention meter");
        let meter = tenant_retention.entry(doc.tenant_id.clone()).or_default();
        let next_tenant_bytes = if delta >= 0 {
            let requested = delta as u64;
            let next = meter
                .0
                .checked_add(requested)
                .ok_or_else(|| BrainError::Journal("tenant journal meter overflowed".into()))?;
            if next > retention_limits.tenant_bytes {
                return Err(BrainError::TenantJournalQuotaExceeded {
                    requested,
                    limit: retention_limits.tenant_bytes,
                });
            }
            next
        } else {
            meter.0.checked_sub(delta.unsigned_abs()).ok_or_else(|| {
                BrainError::Journal("tenant journal meter would become negative".into())
            })?
        };
        let current = map
            .get_mut(session_id)
            .expect("session remains under memory journal lock");
        current.doc = doc.clone();
        current.fence = next_fence;
        current.last_seq = sequence;
        current.retention = next_retention;
        current.owner = None;
        current.lease_expires_ms = 0;
        current.records.insert(sequence, (now_ms, record));
        meter.0 = next_tenant_bytes;
        if let Some(parent_id) = &doc.parent_id {
            self.child_links
                .lock()
                .expect("memory child links")
                .entry(parent_id.clone())
                .or_default()
                .insert(
                    session_id.to_owned(),
                    SessionSummary::from_head(session_id, &doc),
                );
        }
        Ok(EndFence {
            head: Head {
                session_id: session_id.to_owned(),
                doc,
                fence: next_fence,
                last_seq: sequence,
                retention: next_retention,
            },
            newly_fenced: true,
        })
    }

    async fn get_head(&self, session_id: &str) -> Result<Head> {
        let map = self.sessions.lock().expect("memory journal");
        let s = map
            .get(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        Ok(Head {
            session_id: session_id.to_string(),
            doc: s.doc.clone(),
            fence: s.fence,
            last_seq: s.last_seq,
            retention: s.retention,
        })
    }

    async fn read_record_page(&self, query: &RecordPageQuery<'_>) -> Result<RecordPage> {
        let (limit, max_bytes) = validate_record_page_query(query)?;
        let map = self.sessions.lock().expect("memory journal");
        let s = map
            .get(query.session_id)
            .ok_or_else(|| BrainError::NoSuchSession(query.session_id.into()))?;
        if query.after >= query.through_seq {
            return Ok(RecordPage {
                entries: Vec::new(),
                next_after: None,
            });
        }
        let mut entries = Vec::new();
        let mut bytes = 0usize;
        let mut more = false;
        for (seq, (ts_ms, record)) in s
            .records
            .range(query.after.saturating_add(1)..=query.through_seq)
        {
            let record_bytes = serde_json::to_vec(record)?.len();
            if entries.len() >= limit || bytes.saturating_add(record_bytes) > max_bytes {
                more = true;
                break;
            }
            bytes = bytes.saturating_add(record_bytes);
            entries.push(Entry {
                seq: *seq,
                ts_ms: *ts_ms,
                record: record.clone(),
            });
        }
        let next_after = more.then(|| entries.last().expect("page limit admits one record").seq);
        Ok(RecordPage {
            entries,
            next_after,
        })
    }

    async fn commit(&self, decision: &CommitDecision<'_>) -> Result<()> {
        let &CommitDecision {
            session_id,
            owner,
            fence,
            records,
            doc,
            high_water,
            now_ms,
            tenant_storage_delta,
            tenant_storage_limit,
            retention,
            tenant_retention_delta,
            retention_limits,
        } = decision;
        validate_decision(session_id, records, doc)?;
        let mut map = self.sessions.lock().expect("memory journal");
        if requires_ancestor_admission(records) {
            for ancestor_id in &doc.ancestor_ids {
                let ancestor = map
                    .get(ancestor_id)
                    .ok_or_else(|| BrainError::NoSuchSession(ancestor_id.clone()))?;
                if ancestor.doc.root_id != doc.root_id || !child_admission_open(&ancestor.doc) {
                    return Err(BrainError::Fenced);
                }
            }
        }
        let current = map
            .get(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        if current.fence != fence || current.owner.as_deref() != Some(owner) {
            return Err(BrainError::Fenced);
        }
        if records
            .iter()
            .any(|(seq, _)| current.records.contains_key(seq))
        {
            return Err(BrainError::Fenced);
        }
        let expected_retention =
            project_retention(current.retention, records, retention_limits.session_bytes)?;
        if retention != expected_retention
            || tenant_retention_delta != retention_delta(current.retention, retention)?
        {
            return Err(BrainError::Journal(
                "journal retention transition does not match the canonical charge".into(),
            ));
        }
        let mut tenant_storage = self
            .tenant_storage
            .lock()
            .expect("memory tenant storage meter");
        let used = tenant_storage.get(&doc.tenant_id).copied().unwrap_or(0);
        let next_tenant_storage = if tenant_storage_delta >= 0 {
            let requested = tenant_storage_delta as u64;
            let next = used
                .checked_add(requested)
                .ok_or_else(|| BrainError::Journal("tenant storage meter overflowed".into()))?;
            if next > tenant_storage_limit {
                return Err(BrainError::TenantStorageQuotaExceeded {
                    requested,
                    limit: tenant_storage_limit,
                });
            }
            next
        } else {
            let released = tenant_storage_delta.unsigned_abs();
            used.checked_sub(released).ok_or_else(|| {
                BrainError::Journal("tenant storage meter would become negative".into())
            })?
        };
        let mut tenant_retention = self
            .tenant_retention
            .lock()
            .expect("memory tenant retention meter");
        let meter = tenant_retention.entry(doc.tenant_id.clone()).or_default();
        let next_tenant_journal = if tenant_retention_delta >= 0 {
            let requested = tenant_retention_delta as u64;
            let next = meter
                .0
                .checked_add(requested)
                .ok_or_else(|| BrainError::Journal("tenant journal meter overflowed".into()))?;
            if next > retention_limits.tenant_bytes {
                return Err(BrainError::TenantJournalQuotaExceeded {
                    requested,
                    limit: retention_limits.tenant_bytes,
                });
            }
            next
        } else {
            meter
                .0
                .checked_sub(tenant_retention_delta.unsigned_abs())
                .ok_or_else(|| {
                    BrainError::Journal("tenant journal meter would become negative".into())
                })?
        };
        let s = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        for (seq, record) in records {
            s.records.insert(*seq, (now_ms, record.clone()));
        }
        s.doc = doc.clone();
        s.retention = retention;
        s.last_seq = high_water;
        s.lease_expires_ms = now_ms + LEASE_MS; // renew; deliberately no fence bump
        tenant_storage.insert(doc.tenant_id.clone(), next_tenant_storage);
        meter.0 = next_tenant_journal;
        if let Some(parent_id) = &doc.parent_id {
            self.child_links
                .lock()
                .expect("memory child links")
                .entry(parent_id.clone())
                .or_default()
                .insert(
                    session_id.to_owned(),
                    SessionSummary::from_head(session_id, doc),
                );
        }
        Ok(())
    }

    async fn release(&self, session_id: &str, owner: &str, fence: u64) -> Result<()> {
        let mut map = self.sessions.lock().expect("memory journal");
        if let Some(s) = map.get_mut(session_id)
            && s.fence == fence
            && s.owner.as_deref() == Some(owner)
        {
            s.owner = None;
            s.lease_expires_ms = 0;
        }
        Ok(())
    }

    async fn release_and_schedule(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        doc: &HeadDoc,
        due_ms: u64,
    ) -> Result<()> {
        let mut map = self.sessions.lock().expect("memory journal");
        let session = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        if session.fence != fence || session.owner.as_deref() != Some(owner) {
            return Err(BrainError::Fenced);
        }
        session.doc = doc.clone();
        session.doc.recovery_due_ms = Some(due_ms);
        session.owner = None;
        session.lease_expires_ms = 0;
        Ok(())
    }

    async fn renew(
        &self,
        session_id: &str,
        owner: &str,
        fence: u64,
        now_ms: u64,
        recovery_due_ms: Option<u64>,
    ) -> Result<()> {
        let mut map = self.sessions.lock().expect("memory journal");
        let session = map
            .get_mut(session_id)
            .ok_or_else(|| BrainError::NoSuchSession(session_id.into()))?;
        if session.fence != fence || session.owner.as_deref() != Some(owner) {
            return Err(BrainError::Fenced);
        }
        session.lease_expires_ms = now_ms.saturating_add(LEASE_MS);
        if let Some(recovery_due_ms) = recovery_due_ms {
            session.doc.recovery_due_ms = Some(recovery_due_ms);
        }
        Ok(())
    }

    async fn purge_history(&self, session_id: &str) -> Result<u64> {
        let mut map = self.sessions.lock().expect("memory journal");
        let Some(session) = map.get_mut(session_id) else {
            return Ok(0);
        };
        let removed = session.records.len() as u64;
        session.records.clear();
        let sandboxes = self
            .sandboxes
            .lock()
            .expect("memory sandbox inventory")
            .remove(session_id)
            .map_or(0, |items| items.len() as u64);
        Ok(removed.saturating_add(sandboxes))
    }

    async fn put_deletion_status(&self, status: &DeletionStatusDoc) -> Result<()> {
        let mut deletions = self.deletions.lock().expect("memory deletion jobs");
        if deletions.get(&status.session_id).is_some_and(|existing| {
            existing.state == DeletionState::Succeeded && status.state != DeletionState::Succeeded
        }) {
            return Ok(());
        }
        deletions.insert(status.session_id.clone(), status.clone());
        Ok(())
    }

    async fn get_deletion_status(&self, session_id: &str) -> Result<Option<DeletionStatusDoc>> {
        let mut deletions = self.deletions.lock().expect("memory deletion jobs");
        if deletions.get(session_id).is_some_and(|status| {
            status.state == DeletionState::Succeeded && status.expires_at_ms <= crate::wall_ms()
        }) {
            deletions.remove(session_id);
        }
        Ok(deletions.get(session_id).cloned())
    }

    async fn finalize_deletion(&self, status: &DeletionStatusDoc) -> Result<()> {
        let mut deletions = self.deletions.lock().expect("memory deletion jobs");
        if deletions
            .get(&status.session_id)
            .is_some_and(|existing| existing.state == DeletionState::Succeeded)
        {
            return Ok(());
        }
        let mut sessions = self.sessions.lock().expect("memory journal");
        let mut meters = self
            .tenant_storage
            .lock()
            .expect("memory tenant storage meter");
        let mut retention_meters = self
            .tenant_retention
            .lock()
            .expect("memory tenant retention meter");
        let next_meter = if let Some(session) = sessions.get(&status.session_id) {
            if session.doc.tenant_id != status.tenant_id
                || session.doc.tenant_metered_storage_bytes != status.metered_storage_bytes
                || session.retention.metered_bytes != status.metered_journal_bytes
            {
                return Err(BrainError::Journal(
                    "deletion status tenant meter anchor does not match HEAD".into(),
                ));
            }
            let used = meters.get(&status.tenant_id).copied().unwrap_or(0);
            Some(
                used.checked_sub(status.metered_storage_bytes)
                    .ok_or_else(|| {
                        BrainError::Journal("tenant storage meter would become negative".into())
                    })?,
            )
        } else if status.metered_storage_bytes == 0 {
            None
        } else {
            return Err(BrainError::Journal(
                "deletion lost its metered session anchor before final release".into(),
            ));
        };
        let next_retention_meter = if sessions.contains_key(&status.session_id) {
            let (bytes, identities) = retention_meters
                .get(&status.tenant_id)
                .copied()
                .unwrap_or((0, 0));
            Some((
                bytes
                    .checked_sub(status.metered_journal_bytes)
                    .ok_or_else(|| {
                        BrainError::Journal("tenant journal meter would become negative".into())
                    })?,
                identities.checked_sub(1).ok_or_else(|| {
                    BrainError::Journal(
                        "tenant retained-session meter would become negative".into(),
                    )
                })?,
            ))
        } else if status.metered_journal_bytes == 0 {
            None
        } else {
            return Err(BrainError::Journal(
                "deletion lost its retained-session anchor before final release".into(),
            ));
        };
        sessions.remove(&status.session_id);
        if let Some(parent_id) = &status.parent_id {
            if parent_id == &status.root_id {
                if let Some(root) = sessions.get_mut(parent_id) {
                    root.direct_children = root.direct_children.saturating_sub(1);
                    root.descendants = root.descendants.saturating_sub(1);
                }
            } else {
                if let Some(parent) = sessions.get_mut(parent_id) {
                    parent.direct_children = parent.direct_children.saturating_sub(1);
                }
                if let Some(root) = sessions.get_mut(&status.root_id) {
                    root.descendants = root.descendants.saturating_sub(1);
                }
            }
        }
        if let Some(next) = next_meter {
            meters.insert(status.tenant_id.clone(), next);
        }
        if let Some(next) = next_retention_meter {
            retention_meters.insert(status.tenant_id.clone(), next);
        }
        drop(retention_meters);
        drop(meters);
        deletions.insert(status.session_id.clone(), status.clone());
        drop(sessions);
        drop(deletions);
        let mut child_links = self.child_links.lock().expect("memory child links");
        child_links.remove(&status.session_id);
        if let Some(parent_id) = &status.parent_id
            && let Some(children) = child_links.get_mut(parent_id)
        {
            children.remove(&status.session_id);
            if children.is_empty() {
                child_links.remove(parent_id);
            }
        }
        Ok(())
    }

    async fn list_session_page(&self, query: &SessionListQuery<'_>) -> Result<SessionPage> {
        let map = self.sessions.lock().expect("memory journal");
        let mut sessions: Vec<_> = map
            .iter()
            .filter(|(_, session)| session.doc.tenant_id == query.tenant_id)
            .filter(|(_, session)| query.state.is_none_or(|state| session.doc.state == state))
            .map(|(session_id, session)| SessionSummary::from_head(session_id, &session.doc))
            .collect();
        sessions.sort_by(|left, right| {
            right
                .updated_ms
                .cmp(&left.updated_ms)
                .then_with(|| left.session_id.cmp(&right.session_id))
        });
        if let Some(cursor) = query.cursor {
            session_id_from_list_cursor(cursor)?;
            sessions.retain(|session| {
                tenant_session_sort_key(session.updated_ms, &session.session_id).as_str() > cursor
            });
        }
        let has_more = sessions.len() > query.limit;
        sessions.truncate(query.limit);
        let next_cursor = has_more.then(|| {
            let last = sessions.last().expect("a page with more rows is non-empty");
            tenant_session_sort_key(last.updated_ms, &last.session_id)
        });
        Ok(SessionPage {
            sessions,
            next_cursor,
        })
    }

    async fn list_child_page(&self, query: &ChildListQuery<'_>) -> Result<ChildPage> {
        let links = self.child_links.lock().expect("memory child links");
        let Some(children) = links.get(query.parent_id) else {
            return Ok(ChildPage {
                sessions: Vec::new(),
                next_cursor: None,
            });
        };
        let mut rows = children
            .iter()
            .filter(|(child_id, _)| query.cursor.is_none_or(|cursor| child_id.as_str() > cursor))
            .map(|(_, summary)| summary.clone())
            .take(query.limit.clamp(1, 100) + 1)
            .collect::<Vec<_>>();
        let has_more = rows.len() > query.limit.clamp(1, 100);
        rows.truncate(query.limit.clamp(1, 100));
        let next_cursor = has_more.then(|| {
            rows.last()
                .expect("non-empty child page")
                .session_id
                .clone()
        });
        Ok(ChildPage {
            sessions: rows,
            next_cursor,
        })
    }

    async fn reserve_sandbox(
        &self,
        request: &SandboxReserveRequest,
    ) -> Result<SandboxInventoryDoc> {
        let mut sessions = self.sessions.lock().expect("memory journal");
        let root = sessions
            .get_mut(&request.root_id)
            .ok_or_else(|| BrainError::NoSuchSession(request.root_id.clone()))?;
        if root.doc.root_id != request.root_id || !child_admission_open(&root.doc) {
            return Err(BrainError::Invalid(
                "additional sandbox admission is closed for this root".into(),
            ));
        }
        let mut inventories = self.sandboxes.lock().expect("memory sandbox inventory");
        let inventory = inventories.entry(request.root_id.clone()).or_default();
        if let Some(existing) = inventory.get(&request.sandbox_id) {
            if existing.operation_id == request.operation_id
                && existing.request_digest == request.request_digest
                && existing.owner_session_id == request.owner_session_id
            {
                return Ok(existing.clone());
            }
            return Err(BrainError::IdempotencyConflict);
        }
        if root.live_sandboxes >= root.doc.prefix.max_additional_sandboxes_per_root {
            return Err(BrainError::SandboxResourceExhausted);
        }
        root.live_sandboxes = root.live_sandboxes.saturating_add(1);
        let doc = SandboxInventoryDoc {
            root_id: request.root_id.clone(),
            owner_session_id: request.owner_session_id.clone(),
            sandbox_id: request.sandbox_id.clone(),
            operation_id: request.operation_id.clone(),
            request_digest: request.request_digest.clone(),
            generation_intent: request.generation_intent.clone(),
            status: request.initial_status.clone(),
            created_at_ms: request.now_ms,
            updated_at_ms: request.now_ms,
            version: 1,
            slot_released: false,
        };
        inventory.insert(request.sandbox_id.clone(), doc.clone());
        Ok(doc)
    }

    async fn get_sandbox(&self, root_id: &str, sandbox_id: &str) -> Result<SandboxInventoryDoc> {
        self.sandboxes
            .lock()
            .expect("memory sandbox inventory")
            .get(root_id)
            .and_then(|inventory| inventory.get(sandbox_id))
            .cloned()
            .ok_or_else(|| BrainError::FileNotFound(format!("sandbox {sandbox_id}")))
    }

    async fn list_sandbox_page(&self, query: &SandboxListQuery<'_>) -> Result<SandboxPage> {
        let inventories = self.sandboxes.lock().expect("memory sandbox inventory");
        let Some(inventory) = inventories.get(query.root_id) else {
            return Ok(SandboxPage {
                sandboxes: Vec::new(),
                next_cursor: None,
            });
        };
        let limit = query.limit.clamp(1, 100);
        let mut rows = inventory
            .iter()
            .filter(|(sandbox_id, _)| {
                query
                    .cursor
                    .is_none_or(|cursor| sandbox_id.as_str() > cursor)
            })
            .map(|(_, sandbox)| sandbox.clone())
            .take(limit + 1)
            .collect::<Vec<_>>();
        let has_more = rows.len() > limit;
        rows.truncate(limit);
        let next_cursor = has_more.then(|| {
            rows.last()
                .expect("sandbox page with more rows is non-empty")
                .sandbox_id
                .clone()
        });
        Ok(SandboxPage {
            sandboxes: rows,
            next_cursor,
        })
    }

    async fn update_sandbox(&self, request: &SandboxUpdateRequest) -> Result<SandboxInventoryDoc> {
        let mut sessions = self.sessions.lock().expect("memory journal");
        let root = sessions
            .get_mut(&request.root_id)
            .ok_or_else(|| BrainError::NoSuchSession(request.root_id.clone()))?;
        let mut inventories = self.sandboxes.lock().expect("memory sandbox inventory");
        let item = inventories
            .get_mut(&request.root_id)
            .and_then(|inventory| inventory.get_mut(&request.sandbox_id))
            .ok_or_else(|| BrainError::FileNotFound(format!("sandbox {}", request.sandbox_id)))?;
        if item.version != request.expected_version {
            if serde_json::to_value(&item.status)? == serde_json::to_value(&request.status)? {
                return Ok(item.clone());
            }
            return Err(BrainError::Fenced);
        }
        if serde_json::to_value(&item.status.target)?
            != serde_json::to_value(&request.status.target)?
        {
            return Err(BrainError::Journal(
                "sandbox lifecycle update changed its sealed target".into(),
            ));
        }
        if item.slot_released && !request.release_slot {
            return Err(BrainError::SandboxGone);
        }
        if request.release_slot
            && !matches!(
                request.status.state,
                brain_protocol::hand::SandboxState::Gone
                    | brain_protocol::hand::SandboxState::Terminated
            )
        {
            return Err(BrainError::Journal(
                "sandbox slot may be released only for a confirmed terminal target".into(),
            ));
        }
        if request.release_slot && !item.slot_released {
            root.live_sandboxes = root.live_sandboxes.saturating_sub(1);
            item.slot_released = true;
        }
        item.status = request.status.clone();
        item.updated_at_ms = request.now_ms;
        item.version = item.version.saturating_add(1);
        Ok(item.clone())
    }

    async fn list_recovery_page(&self, query: &RecoveryQuery<'_>) -> Result<RecoveryPage> {
        let map = self.sessions.lock().expect("memory journal");
        let mut candidates =
            map.iter()
                .filter_map(|(session_id, session)| {
                    let due_ms = session.doc.recovery_due_ms?;
                    (recovery_shard(session_id) == query.shard && due_ms <= query.due_before_ms)
                        .then(|| {
                            (
                                recovery_due_key(due_ms, session_id),
                                RecoveryItem {
                                    session_id: session_id.clone(),
                                    due_ms,
                                    state: session.doc.state,
                                    active_phase: session.doc.active_phase,
                                    last_seq: session.last_seq,
                                    root_id: session.doc.root_id.clone(),
                                    parent_id: session.doc.parent_id.clone(),
                                    updated_ms: session.doc.updated_ms,
                                },
                            )
                        })
                })
                .filter(|(key, _)| query.cursor.is_none_or(|cursor| key.as_str() > cursor))
                .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.0.cmp(&right.0));
        let limit = query.limit.clamp(1, 100);
        let has_more = candidates.len() > limit;
        candidates.truncate(limit);
        let next_cursor = has_more.then(|| candidates.last().expect("non-empty page").0.clone());
        Ok(RecoveryPage {
            items: candidates.into_iter().map(|(_, item)| item).collect(),
            next_cursor,
        })
    }

    async fn list_sessions(&self, limit: usize) -> Result<Vec<Head>> {
        let map = self.sessions.lock().expect("memory journal");
        Ok(map
            .iter()
            .take(limit)
            .map(|(sid, s)| Head {
                session_id: sid.clone(),
                doc: s.doc.clone(),
                fence: s.fence,
                last_seq: s.last_seq,
                retention: s.retention,
            })
            .collect())
    }
}

#[doc(hidden)]
pub fn child_admission_open(doc: &HeadDoc) -> bool {
    !doc.ended
        && !matches!(
            doc.state.as_str(),
            "ending" | "ended" | "deleting" | "deleted" | "failed"
        )
}

/// Build the one-record, constant-size END projection from a strong HEAD snapshot. Adapters use
/// this inside their own atomic compare-and-swap transaction. A concurrent commit either lands
/// before this snapshot and is retained, or loses the fence after this transition; there is no
/// interval in which the old owner is fenced while descendants still observe admission open.
pub fn project_end_fence(head: &Head, now_ms: u64) -> Result<Option<(HeadDoc, u64, Record)>> {
    if matches!(
        head.doc.state.as_str(),
        "ending" | "ended" | "deleting" | "deleted"
    ) {
        return Ok(None);
    }
    let sequence = head
        .last_seq
        .checked_add(1)
        .ok_or_else(|| BrainError::Journal("journal sequence exhausted".into()))?;
    let mut doc = head.doc.clone();
    doc.ended = true;
    doc.state = SessionLifecycle::Ending;
    doc.updated_ms = now_ms;
    doc.last_seq = sequence;
    doc.recovery_attempt = 0;
    doc = doc.with_recovery_projection(now_ms);
    let record = Record::State {
        state: SessionLifecycle::Ending,
        turn: doc.turn.clone(),
    };
    validate_decision(&head.session_id, &[(sequence, record.clone())], &doc)?;
    Ok(Some((doc, sequence, record)))
}

/// A decision that starts a new turn must atomically observe every immutable ancestor fence.
/// Recovery/terminal commits deliberately do not use this predicate: an ancestor ending while a
/// child effect is already in flight must not prevent the child from recording its exact outcome.
pub fn requires_ancestor_admission(records: &[(u64, Record)]) -> bool {
    records
        .iter()
        .any(|(_, record)| matches!(record, Record::TurnStarted { .. }))
}

pub fn validate_ancestor_path(doc: &HeadDoc) -> Result<()> {
    match &doc.parent_id {
        None if doc.depth == 0 && doc.ancestor_ids.is_empty() && !doc.root_id.is_empty() => Ok(()),
        Some(parent_id)
            if doc.depth as usize == doc.ancestor_ids.len()
                && doc
                    .ancestor_ids
                    .last()
                    .is_some_and(|value| value == parent_id)
                && doc
                    .ancestor_ids
                    .first()
                    .is_some_and(|value| value == &doc.root_id)
                && doc.ancestor_ids.len() <= 8 =>
        {
            Ok(())
        }
        _ => Err(BrainError::Invalid(
            "session ancestor path does not match root, parent and depth".into(),
        )),
    }
}
