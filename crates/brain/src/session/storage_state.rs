use super::*;

pub(super) async fn expire_storage_upload(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
) -> Result<()> {
    let _heartbeat = start_lease_heartbeat(brain, session_id, &st.lease, true, None);
    let Some(upload) = st.head.storage_upload.clone() else {
        return Ok(());
    };
    if upload.state == UploadReservationState::Completed {
        return Ok(());
    }
    let storage = brain.storage_port()?.clone();
    if upload.state == UploadReservationState::Published {
        let object = storage.stat(session_id, &upload.key).await?;
        if !matches_storage_publication(&object, &upload) {
            return Err(BrainError::Journal(
                "published storage object does not match its durable reservation".into(),
            ));
        }
        storage
            .abort_upload(session_id, &upload.transfer_id)
            .await?;
        let mut completed = upload.clone();
        completed.state = UploadReservationState::Completed;
        st.head.storage_reserved_bytes = 0;
        st.head.storage_upload = Some(completed);
        let seq = st.take_seq();
        return commit(
            brain,
            session_id,
            st,
            vec![(
                seq,
                Record::StorageUploadCompleted {
                    transfer_id: upload.transfer_id,
                    key: upload.key,
                    bytes: upload.bytes,
                    published_bytes: st.head.session_storage_bytes,
                    reserved_bytes: st.head.storage_reserved_bytes,
                },
            )],
        )
        .await;
    }
    if upload.state == UploadReservationState::Reserved {
        match storage.stat(session_id, &upload.key).await {
            Ok(object) if matches_storage_publication(&object, &upload) => {
                // `complete_upload` may have copied destination bytes before its response or the
                // following journal decision was lost. Adopt that exact sealed result before
                // touching staging, even after ticket expiry; otherwise visible bytes would be
                // left unmetered while the reservation was released.
                st.head.session_storage_bytes = st
                    .head
                    .session_storage_bytes
                    .saturating_sub(upload.previous_bytes)
                    .checked_add(upload.bytes)
                    .ok_or_else(|| {
                        BrainError::Journal("session storage meter overflowed".into())
                    })?;
                st.head.storage_reserved_bytes = 0;
                let mut published = upload.clone();
                published.sha256 = Some(object.sha256.clone());
                published.state = UploadReservationState::Published;
                st.head.storage_upload = Some(published);
                let seq = st.take_seq();
                commit(
                    brain,
                    session_id,
                    st,
                    vec![(
                        seq,
                        Record::StorageUploadPublished {
                            transfer_id: upload.transfer_id.clone(),
                            key: upload.key.clone(),
                            bytes: upload.bytes,
                            published_bytes: st.head.session_storage_bytes,
                            reserved_bytes: st.head.storage_reserved_bytes,
                        },
                    )],
                )
                .await?;
                // Publication is now durable. Staging cleanup may be retried independently and
                // cannot make the published object or its tenant charge disappear.
                storage
                    .abort_upload(session_id, &upload.transfer_id)
                    .await?;
                let mut completed = upload.clone();
                completed.sha256 = Some(object.sha256.clone());
                completed.state = UploadReservationState::Completed;
                st.head.storage_upload = Some(completed);
                let seq = st.take_seq();
                return commit(
                    brain,
                    session_id,
                    st,
                    vec![(
                        seq,
                        Record::StorageUploadCompleted {
                            transfer_id: upload.transfer_id,
                            key: upload.key,
                            bytes: upload.bytes,
                            published_bytes: st.head.session_storage_bytes,
                            reserved_bytes: st.head.storage_reserved_bytes,
                        },
                    )],
                )
                .await;
            }
            Err(BrainError::FileNotFound(_)) => {}
            // An overwrite may legitimately see the prior visible object before the staging
            // copy runs. Equality of bytes/hash is not publication proof; only the intent id is.
            Ok(_) if upload.overwrite => {}
            Ok(_) => {
                return Err(BrainError::Journal(
                    "storage destination conflicts with its durable upload reservation".into(),
                ));
            }
            Err(error) => return Err(error),
        }
    }
    if upload.expires_at_ms > crate::wall_ms() {
        return Ok(());
    }
    if upload.state == UploadReservationState::InlineReserved {
        match storage.stat(session_id, &upload.key).await {
            Ok(object) if matches_storage_publication(&object, &upload) => {
                st.head.session_storage_bytes = st
                    .head
                    .session_storage_bytes
                    .saturating_sub(upload.previous_bytes)
                    .checked_add(upload.bytes)
                    .ok_or_else(|| {
                        BrainError::Journal("session storage meter overflowed".into())
                    })?;
                st.head.storage_reserved_bytes = 0;
                let mut completed = upload.clone();
                completed.state = UploadReservationState::Completed;
                st.head.storage_upload = Some(completed);
                let seq = st.take_seq();
                return commit(
                    brain,
                    session_id,
                    st,
                    vec![(
                        seq,
                        Record::StorageUploadCompleted {
                            transfer_id: upload.transfer_id,
                            key: upload.key,
                            bytes: upload.bytes,
                            published_bytes: st.head.session_storage_bytes,
                            reserved_bytes: st.head.storage_reserved_bytes,
                        },
                    )],
                )
                .await;
            }
            Err(BrainError::FileNotFound(_)) => {}
            // The pre-existing overwrite target is not evidence that this inline intent ran.
            // At expiry leave it untouched and release only the unpublished reservation.
            Ok(_) if upload.overwrite => {}
            Ok(_) => {
                return Err(BrainError::Journal(
                    "inline storage object conflicts with its durable intent".into(),
                ));
            }
            Err(error) => return Err(error),
        }
    } else if upload.state != UploadReservationState::Reserved {
        return Err(BrainError::Journal(format!(
            "storage upload has invalid state {}",
            upload.state.as_str()
        )));
    }
    // Deletion precedes reservation release. A transient S3 failure therefore leaves the hard
    // bound in place and the next operation retries cleanup instead of admitting more bytes.
    if upload.state == UploadReservationState::Reserved {
        storage
            .abort_upload(session_id, &upload.transfer_id)
            .await?;
    }
    st.head.storage_reserved_bytes = 0;
    st.head.storage_upload = None;
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::StorageUploadExpired {
                transfer_id: upload.transfer_id,
                key: upload.key,
                bytes: upload.bytes,
                published_bytes: st.head.session_storage_bytes,
                reserved_bytes: st.head.storage_reserved_bytes,
            },
        )],
    )
    .await
}

pub(crate) async fn prepare_storage_upload_state(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    intent: crate::storage::StorageUploadIntent,
) -> Result<crate::storage::StorageTransferTicket> {
    prepare_storage_upload_state_for_transfer(brain, session_id, st, intent, None).await
}

pub(super) async fn prepare_storage_upload_state_for_transfer(
    brain: &Arc<Brain>,
    session_id: &str,
    st: &mut TurnState,
    intent: crate::storage::StorageUploadIntent,
    requested_transfer_id: Option<String>,
) -> Result<crate::storage::StorageTransferTicket> {
    ensure_storage_readable(&st.head, session_id)?;
    validate_storage_upload_intent(&intent, st.head.prefix.storage_max_object_bytes)?;
    expire_storage_upload(brain, session_id, st).await?;

    if let Some(upload) = &st.head.storage_upload {
        let same_intent = upload.key == intent.key
            && upload.bytes == intent.bytes
            && (upload.sha256 == intent.sha256
                || (requested_transfer_id.is_some() && intent.sha256.is_none()))
            && upload.content_type == intent.content_type
            && upload.overwrite == intent.overwrite;
        let same_requested_transfer = requested_transfer_id
            .as_deref()
            .is_some_and(|transfer_id| transfer_id == upload.transfer_id);
        if same_intent
            && ((upload.state == UploadReservationState::Reserved
                && (requested_transfer_id.is_none() || same_requested_transfer))
                || (upload.state == UploadReservationState::Completed && same_requested_transfer))
        {
            return brain
                .storage_port()?
                .prepare_upload(crate::storage::StorageUploadRequest {
                    session_id: session_id.to_owned(),
                    transfer_id: upload.transfer_id.clone(),
                    key: upload.key.clone(),
                    bytes: upload.bytes,
                    sha256: upload.sha256.clone(),
                    content_type: upload.content_type.clone(),
                    overwrite: upload.overwrite,
                    expires_at_ms: upload.expires_at_ms,
                })
                .await;
        }
        if upload.state != UploadReservationState::Completed {
            return Err(BrainError::StorageUploadInProgress {
                transfer_id: upload.transfer_id.clone(),
            });
        }
    }

    let storage = brain.storage_port()?.clone();
    let previous_bytes = match storage.stat(session_id, &intent.key).await {
        Ok(object) if intent.overwrite => object.bytes,
        Ok(_) => {
            return Err(BrainError::Invalid(format!(
                "session storage object {} already exists",
                intent.key
            )));
        }
        Err(BrainError::FileNotFound(_)) => 0,
        Err(error) => return Err(error),
    };
    let visible_after_publish = st
        .head
        .session_storage_bytes
        .saturating_sub(previous_bytes)
        .checked_add(intent.bytes)
        .ok_or_else(|| BrainError::Invalid("session storage byte count overflowed".into()))?;
    // Completion briefly retains both the verified staging object and its published copy so a
    // lost response remains retryable. Keep that worst-case physical footprint within quota.
    let peak_bytes = visible_after_publish
        .checked_add(intent.bytes)
        .ok_or_else(|| BrainError::Invalid("session storage byte count overflowed".into()))?;
    if peak_bytes > st.head.prefix.storage_max_session_bytes {
        return Err(BrainError::StorageQuotaExceeded {
            published: st.head.session_storage_bytes,
            reserved: st.head.storage_reserved_bytes,
            requested: intent.bytes,
            limit: st.head.prefix.storage_max_session_bytes,
        });
    }

    let transfer_id = requested_transfer_id.unwrap_or_else(|| crate::mint_id("xfer", 24));
    let expires_at_ms = crate::wall_ms()
        .checked_add(st.head.prefix.storage_transfer_ttl_ms)
        .ok_or_else(|| BrainError::Invalid("storage transfer expiry overflowed".into()))?;
    let upload = StorageUploadReservationDoc {
        transfer_id: transfer_id.clone(),
        key: intent.key.clone(),
        bytes: intent.bytes,
        sha256: intent.sha256.clone(),
        content_type: intent.content_type.clone(),
        overwrite: intent.overwrite,
        previous_bytes,
        expires_at_ms,
        state: UploadReservationState::Reserved,
    };
    st.head.storage_reserved_bytes = intent.bytes;
    st.head.storage_upload = Some(upload.clone());
    let storage_gauges = (
        st.head.session_storage_bytes,
        st.head.storage_reserved_bytes,
    );
    let seq = st.take_seq();
    commit(
        brain,
        session_id,
        st,
        vec![(
            seq,
            Record::StorageUploadReserved {
                transfer_id: transfer_id.clone(),
                key: intent.key.clone(),
                bytes: intent.bytes,
                sha256: intent.sha256.clone(),
                expires_at_ms,
                published_bytes: storage_gauges.0,
                reserved_bytes: storage_gauges.1,
            },
        )],
    )
    .await?;

    storage
        .prepare_upload(crate::storage::StorageUploadRequest {
            session_id: session_id.to_owned(),
            transfer_id,
            key: intent.key,
            bytes: intent.bytes,
            sha256: intent.sha256,
            content_type: intent.content_type,
            overwrite: intent.overwrite,
            expires_at_ms,
        })
        .await
}

pub(super) async fn do_complete_storage_upload(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    transfer_id: String,
) -> Result<crate::storage::StorageObject> {
    let r = ensure_resident(brain, session_id, resident).await?;
    complete_storage_upload_state(brain, session_id, &mut r.st, transfer_id).await
}

pub(super) async fn do_write_storage_inline(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    key: String,
    content_base64: String,
    content_type: Option<String>,
    overwrite: bool,
) -> Result<crate::storage::StorageObject> {
    let r = ensure_resident(brain, session_id, resident).await?;
    write_storage_inline_state(
        brain,
        session_id,
        &mut r.st,
        key,
        content_base64,
        content_type,
        overwrite,
    )
    .await
}

pub(super) async fn do_delete_storage_object(
    brain: &Arc<Brain>,
    session_id: &str,
    resident: &mut Option<Resident>,
    key: String,
) -> Result<()> {
    crate::storage::validate_storage_adapter_key(&key)?;
    let r = ensure_resident(brain, session_id, resident).await?;
    ensure_storage_readable(&r.st.head, session_id)?;
    reconcile_storage_mutations(brain, session_id, &mut r.st).await?;
    if let Some(upload) = &r.st.head.storage_upload
        && upload.state != UploadReservationState::Completed
    {
        return Err(BrainError::StorageUploadInProgress {
            transfer_id: upload.transfer_id.clone(),
        });
    }
    if r.st.head.storage_delete.is_some() {
        reconcile_storage_delete(brain, session_id, &mut r.st).await?;
    }
    let storage = brain.storage_port()?.clone();
    let object = match storage.stat(session_id, &key).await {
        Ok(object) => object,
        Err(BrainError::FileNotFound(_)) => return Ok(()),
        Err(error) => return Err(error),
    };
    let operation_id = crate::mint_id("del", 24);
    r.st.head.storage_delete = Some(StorageDeleteReservationDoc {
        operation_id: operation_id.clone(),
        key: key.clone(),
        bytes: object.bytes,
        sha256: object.sha256.clone(),
    });
    let storage_gauges = (
        r.st.head.session_storage_bytes,
        r.st.head.storage_reserved_bytes,
    );
    let seq = r.st.take_seq();
    commit(
        brain,
        session_id,
        &mut r.st,
        vec![(
            seq,
            Record::StorageDeleteIntent {
                operation_id,
                key,
                bytes: object.bytes,
                sha256: object.sha256,
                published_bytes: storage_gauges.0,
                reserved_bytes: storage_gauges.1,
            },
        )],
    )
    .await?;
    reconcile_storage_delete(brain, session_id, &mut r.st).await
}
