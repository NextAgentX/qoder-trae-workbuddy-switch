//! WorkBuddy session-link storage migration and workbench API adapters.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{json, Value};

use crate::modules::account;
use crate::modules::config::now_ms;
use crate::modules::linked_workbuddy::{self, SessionPaths, WbVariant};
use crate::modules::region::Region;
use crate::modules::session_link::{
    self, LinkGroup, LinkMember, LinkStore, MemberState, StoreState,
};

use super::{
    baseline_key, build_projects_index_for, copy_ledger_path_for, current_user_uid_for,
    fingerprint_of_text, group_fingerprint, item_group_key, ledger_new_of,
    load_session_baselines_at, open_db, parse_group_baselines, render_replica_groups,
    replica_endpoint_is_live, replica_group_entry, session_baseline_path_for,
    workbuddy_db_path_for, PairBaseline, LEDGER_SEP,
};

type LegacyReplica = (String, String, String);

/// Import connected components from both old ledgers under the new store lock.
/// A successfully initialized store is authoritative, including after unlinking.
pub(crate) fn ensure_link_store() -> Result<SessionPaths, String> {
    let paths = SessionPaths::for_variant(WbVariant::Cn);
    match session_link::load_store(&paths) {
        StoreState::Ready(_) => return Ok(paths),
        StoreState::Unavailable(reason) => return Err(reason),
        StoreState::Missing => {}
    }
    session_link::with_link_store_write(&paths, |store| {
        if store.revision != 0 || !store.groups.is_empty() {
            return Ok(());
        }
        let mut ledgers = Vec::new();
        for region in Region::all() {
            let path = copy_ledger_path_for(region);
            let source = match std::fs::read_to_string(&path) {
                Ok(source) => source,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(format!("无法读取旧会话账本 {}：{error}", path.display()))
                }
            };
            let ledger = serde_json::from_str(&source)
                .map_err(|error| format!("旧会话账本 {} 无法解析：{error}", path.display()))?;
            ledgers.push((region, ledger));
        }
        import_legacy_links(store, &ledgers);
        Ok(())
    })?;
    Ok(paths)
}

fn import_legacy_links(
    store: &mut LinkStore,
    ledgers: &[(Region, serde_json::Map<String, Value>)],
) {
    let mut adjacency: BTreeMap<LegacyReplica, BTreeSet<LegacyReplica>> = BTreeMap::new();
    for (region, ledger) in ledgers {
        for (key, value) in ledger {
            let fields: Vec<_> = key.split(LEDGER_SEP).collect();
            let (source_region, source_uid, target_uid, source_cid) = match fields.as_slice() {
                [source_uid, target_uid, source_cid] => {
                    (*region, *source_uid, *target_uid, *source_cid)
                }
                [source_region, source_uid, target_uid, source_cid] => {
                    let Some(source_region) = Region::parse(source_region) else {
                        continue;
                    };
                    (source_region, *source_uid, *target_uid, *source_cid)
                }
                _ => continue,
            };
            let Some(target_cid) = ledger_new_of(value) else {
                continue;
            };
            if [source_uid, target_uid, source_cid, &target_cid]
                .iter()
                .any(|field| field.is_empty())
            {
                continue;
            }
            let source = (
                source_region.as_str().to_owned(),
                source_uid.to_owned(),
                source_cid.to_owned(),
            );
            let target = (
                region.as_str().to_owned(),
                target_uid.to_owned(),
                target_cid,
            );
            if source != target {
                adjacency
                    .entry(source.clone())
                    .or_default()
                    .insert(target.clone());
                adjacency.entry(target).or_default().insert(source);
            }
        }
    }

    let linked_at = now_ms();
    while let Some((first, neighbors)) = adjacency.pop_first() {
        let mut pending = vec![first.clone()];
        pending.extend(neighbors);
        let mut component = BTreeSet::new();
        while let Some(replica) = pending.pop() {
            if let Some(neighbors) = adjacency.remove(&replica) {
                pending.extend(neighbors);
            }
            component.insert(replica);
        }
        let mut uid_counts = HashMap::new();
        for (region, uid, _) in &component {
            *uid_counts
                .entry((region.clone(), uid.clone()))
                .or_insert(0usize) += 1;
        }
        let members = component
            .into_iter()
            .map(|(region, uid, session_id)| {
                // A legacy component can contain two copies for the same uid. Keep
                // both identities and require an explicit choice before writing either.
                let state = if uid_counts[&(region.clone(), uid.clone())] == 1 {
                    MemberState::Active
                } else {
                    MemberState::Stale
                };
                LinkMember {
                    member_id: uuid::Uuid::new_v4().to_string(),
                    account_id: None,
                    uid,
                    session_id,
                    variant: Region::parse(&region).map(WbVariant::for_region),
                    state,
                    linked_at,
                    last_synced_at: None,
                }
            })
            .collect();
        store.groups.push(LinkGroup {
            id: uuid::Uuid::new_v4().to_string(),
            variant: WbVariant::for_region(Region::parse(&first.0).unwrap_or_default()),
            created_at: linked_at,
            members,
            pair_bases: vec![],
        });
    }
}

pub fn prune_linked_replicas_for(region: Region) -> Result<Value, String> {
    let paths = ensure_link_store()?;
    let variant = WbVariant::for_region(region);
    let _locks = WbVariant::ALL
        .into_iter()
        .map(|item| session_link::try_acquire_variant_ops_lock(&paths, item))
        .collect::<Result<Vec<_>, _>>()?;
    let scan = session_link::scan_operations(&paths);
    if !scan.complete
        || !scan.problems.is_empty()
        || scan.operations.iter().any(|op| op.phase.is_unfinished())
    {
        return Err("请先恢复未完成的会话操作".to_string());
    }
    let projects = build_projects_index_for(region);
    let conn = open_db(&workbuddy_db_path_for(region), true);
    session_link::with_link_store_write(&paths, |store| {
        let mut scanned = 0;
        let mut removed = Vec::new();
        for group in &mut store.groups {
            let fallback_variant = group.variant;
            let mut removed_ids = HashSet::new();
            group.members.retain(|member| {
                if member.variant.unwrap_or(fallback_variant) != variant {
                    return true;
                }
                scanned += 1;
                if replica_endpoint_is_live(
                    conn.as_ref(),
                    &projects,
                    &member.uid,
                    &member.session_id,
                ) == Some(false)
                {
                    removed.push(member.member_id.clone());
                    removed_ids.insert(member.member_id.clone());
                    return false;
                }
                true
            });
            group
                .pair_bases
                .retain(|pair| pair.member_ids.iter().all(|id| !removed_ids.contains(id)));
        }
        store.groups.retain(|group| group.members.len() > 1);
        Ok(json!({
            "region": region.as_str(),
            "scanned": scanned,
            "removed": removed.len(),
            "kept": scanned - removed.len(),
            "removedKeys": removed,
        }))
    })
}

/// Workbench contract backed by the versioned association store.
pub fn list_linked_replica_groups_for(region: Region) -> Result<Value, String> {
    let paths = ensure_link_store()?;
    let store = match session_link::load_store(&paths) {
        StoreState::Ready(store) => store,
        StoreState::Unavailable(reason) => return Err(reason),
        StoreState::Missing => return Err("会话关联存储已消失".to_string()),
    };
    let baselines = parse_group_baselines(&load_session_baselines_at(&session_baseline_path_for(
        region,
    )));
    let entries = store
        .groups
        .into_iter()
        .filter_map(|group| {
            let mut members: Vec<_> = group
                .members
                .iter()
                .filter(|member| member.state == MemberState::Active)
                .map(|member| {
                    (
                        (member.uid.clone(), member.session_id.clone()),
                        session_link::member_variant(&group, member).region(),
                    )
                })
                .collect();
            if members.len() < 2
                || !members
                    .iter()
                    .any(|(_, member_region)| *member_region == region)
            {
                return None;
            }
            members.sort_by(|left, right| left.0.cmp(&right.0));
            let (members, regions): (Vec<_>, Vec<_>) = members.into_iter().unzip();
            // The existing view uses session IDs as branch identifiers. Ambiguous
            // historical identities remain in the store and cannot be submitted.
            if members
                .iter()
                .map(|(_, cid)| cid)
                .collect::<HashSet<_>>()
                .len()
                != members.len()
            {
                return None;
            }
            Some(replica_group_entry(group.id, members, regions, &baselines))
        })
        .collect();
    let scan = session_link::scan_operations(&paths);
    let variant = WbVariant::for_region(region);
    let mut result = render_replica_groups(region, entries, "ledger");
    result["needsRecovery"] = json!(
        !scan.complete
            || !scan.problems.is_empty()
            || scan
                .operations
                .iter()
                .any(|op| op.variant == variant && op.phase.is_unfinished())
    );
    Ok(result)
}

pub fn recover_linked_sessions_for(region: Region) -> Result<Value, String> {
    let report =
        linked_workbuddy::recover_pending_session_operations(WbVariant::for_region(region))?;
    Ok(json!({
        "recovered": report.recovered,
        "abandoned": report.abandoned,
        "needsRecovery": report.needs_recovery,
        "temporaryFiles": report.temporary_files,
    }))
}

pub fn manage_linked_sessions_for(region: Region, change: &Value) -> Result<Value, String> {
    let paths = ensure_link_store()?;
    let variant = WbVariant::for_region(region);
    let id = change["groupKey"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("缺少会话关联组")?;
    let action = change["action"].as_str().ok_or("缺少会话关联操作")?;
    let session_id = change["sessionId"].as_str();
    if action == "add" {
        let session_id = session_id.ok_or("请选择来源副本")?;
        let target_id = change["targetAccountId"].as_str().ok_or("请选择目标账号")?;
        let mut target = account::find_account_for(region, target_id).ok_or("目标账号不存在")?;
        let StoreState::Ready(store) = session_link::load_store(&paths) else {
            return Err("会话关联存储不可用".to_string());
        };
        let group = store
            .groups
            .iter()
            .find(|group| group.id == id)
            .ok_or("会话关联组已变更")?;
        if !group
            .members
            .iter()
            .any(|member| session_link::member_variant(group, member) == variant)
        {
            return Err("会话关联不属于当前版本".to_string());
        }
        let source = find_active_link_member(group, session_id)?;
        let source_region = session_link::member_variant(group, source).region();
        let source =
            json!({"uid": source.uid, "id": source.account_id, "region": source_region.as_str()});
        target["region"] = json!(region.as_str());
        let report = linked_workbuddy::copy_sessions_cross_for_group(
            &source,
            &target,
            &[session_id.to_owned()],
            Some(id),
        )?;
        if let Some(error) = report["errors"][0]["error"].as_str() {
            return Err(error.to_owned());
        }
        return Ok(report);
    }
    if !matches!(action, "unlink" | "delete") {
        return Err("不支持的会话关联操作".to_string());
    }
    let _locks = WbVariant::ALL
        .into_iter()
        .map(|item| session_link::try_acquire_variant_ops_lock(&paths, item))
        .collect::<Result<Vec<_>, _>>()?;
    let scan = session_link::scan_operations(&paths);
    if !scan.complete
        || !scan.problems.is_empty()
        || scan
            .operations
            .iter()
            .any(|op| op.group_id == id && op.phase.is_unfinished())
    {
        return Err("请先恢复未完成的会话操作".to_string());
    }
    session_link::with_link_store_write(&paths, |store| {
        let index = store
            .groups
            .iter()
            .position(|group| {
                group.id == id
                    && group
                        .members
                        .iter()
                        .any(|member| session_link::member_variant(group, member) == variant)
            })
            .ok_or("会话关联组已变更")?;
        if action == "delete" {
            store.groups.remove(index);
        } else {
            let group = &mut store.groups[index];
            let session_id = session_id.ok_or("缺少要解除关联的会话")?;
            let id = &find_active_link_member(group, session_id)?.member_id;
            let member = group
                .members
                .iter()
                .position(|member| member.member_id == *id)
                .ok_or("会话副本已变更")?;
            let id = group.members.remove(member).member_id;
            group
                .pair_bases
                .retain(|pair| !pair.member_ids.contains(&id));
            if group.members.len() < 2 {
                store.groups.remove(index);
            }
        }
        Ok(json!({"ok": true}))
    })
}

fn find_active_link_member<'a>(group: &'a LinkGroup, cid: &str) -> Result<&'a LinkMember, String> {
    let mut matches = group
        .members
        .iter()
        .filter(|member| member.session_id == cid && member.state == MemberState::Active);
    let member = matches.next().ok_or("会话副本已变更，请刷新后重试")?;
    if matches.next().is_some() {
        return Err("会话副本身份不唯一".to_string());
    }
    Ok(member)
}

fn restore_legacy_pair_baseline(
    paths: &SessionPaths,
    group: &mut LinkGroup,
    source: &LinkMember,
    target: &LinkMember,
    legacy_baselines: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    if session_link::find_pair_base(group, &source.member_id, &target.member_id).is_some() {
        return Ok(());
    }
    let key = baseline_key(&source.session_id, &target.session_id);
    let Some(legacy) = legacy_baselines
        .get(&key)
        .and_then(|value| serde_json::from_value::<PairBaseline>(value.clone()).ok())
    else {
        return Ok(());
    };
    if legacy.line_count == 0 || legacy.line_count != legacy.prefix_hashes.len() {
        return Ok(());
    }
    let candidates = [source, target]
        .into_iter()
        .filter_map(|member| {
            let member_paths =
                SessionPaths::for_variant(session_link::member_variant(group, member));
            let session_link::ContentState::Ready(snapshot) =
                linked_workbuddy::member_content_state(&member_paths, &member.session_id)
            else {
                return None;
            };
            let hashes = fingerprint_of_text(&snapshot.text, &member.session_id).prefix_hashes;
            if !hashes.starts_with(&legacy.prefix_hashes) {
                return None;
            }
            snapshot
                .normalized
                .line_digests
                .get(..legacy.line_count)
                .map(<[_]>::to_vec)
        })
        .collect::<Vec<_>>();
    let Some(digests) = candidates.first() else {
        return Ok(());
    };
    if candidates.iter().any(|candidate| candidate != digests) {
        return Ok(());
    }
    let baseline_ref = uuid::Uuid::new_v4().to_string();
    session_link::with_link_store_write(paths, |store| {
        let persisted = store
            .groups
            .iter_mut()
            .find(|candidate| candidate.id == group.id)
            .ok_or("会话关联已变更，请刷新后重试")?;
        if session_link::group_fingerprint(persisted) != session_link::group_fingerprint(group) {
            return Err("会话关联已变更，请刷新后重试".to_owned());
        }
        let normalized = session_link::NormalizedContent {
            record_count: digests.len(),
            total_digest: session_link::total_digest_of(digests),
            line_digests: digests.clone(),
        };
        session_link::save_baseline(paths, &baseline_ref, &normalized)?;
        session_link::set_pair_base(
            persisted,
            &source.member_id,
            &target.member_id,
            &baseline_ref,
            session_link::NORMALIZATION_VERSION,
        );
        group.pair_bases = persisted.pair_bases.clone();
        Ok(())
    })
}

pub(crate) fn restore_link_pair_legacy_baseline(
    group_id: &str,
    source_cid: &str,
    target_cid: &str,
) -> Result<(), String> {
    let paths = ensure_link_store()?;
    let StoreState::Ready(mut store) = session_link::load_store(&paths) else {
        return Err("会话关联存储不可用".to_string());
    };
    let group = store
        .groups
        .iter_mut()
        .find(|group| group.id == group_id)
        .ok_or("会话关联组已变更")?;
    let source = find_active_link_member(group, source_cid)?.clone();
    let target = find_active_link_member(group, target_cid)?.clone();
    let variant = session_link::member_variant(group, &source);
    if session_link::member_variant(group, &target) == variant {
        let legacy = load_session_baselines_at(&session_baseline_path_for(variant.region()));
        restore_legacy_pair_baseline(&paths, group, &source, &target, &legacy)?;
    }
    Ok(())
}

/// Run or preview an explicitly selected pair from the current WorkBuddy scope.
pub fn linked_replica_batch(
    region: Region,
    items: &[Value],
    unify: bool,
    preview: bool,
) -> Result<Value, String> {
    use session_link::{SyncMode, SyncVerdict};

    if items.is_empty() {
        return Ok(json!({"results": [], "okCount": 0, "failCount": 0}));
    }
    let paths = ensure_link_store()?;
    let StoreState::Ready(mut store) = session_link::load_store(&paths) else {
        return Err("无法读取会话关联存储".to_string());
    };
    if preview {
        // Prune once before issuing a batch. Every item in this response must
        // survive until the user can confirm it.
        session_link::prune_preview_tokens(&paths, session_link::KEEP_PREVIEW_TOKENS);
    }
    let legacy_baselines = if preview && unify {
        load_session_baselines_at(&session_baseline_path_for(region))
    } else {
        serde_json::Map::new()
    };
    let mut results = Vec::with_capacity(items.len());
    let mut ok_count = 0;
    for item in items {
        let source_cid = item["sourceCid"].as_str().unwrap_or_default();
        let target_cid = item["targetCid"].as_str().unwrap_or_default();
        let mut result = json!({
            "sourceCid": source_cid,
            "targetCid": target_cid,
            "groupKey": item_group_key(item),
            "ok": false,
        });
        let outcome = (|| {
            let group_key = item_group_key(item).ok_or("缺少会话关联组")?;
            let group = store
                .groups
                .iter_mut()
                .find(|group| group.id == group_key)
                .ok_or("会话关联已变更，请刷新后重试")?;
            if !group
                .members
                .iter()
                .any(|member| session_link::member_variant(group, member).region() == region)
            {
                return Err("会话关联不属于当前版本".to_string());
            }
            let source = find_active_link_member(group, source_cid)?.clone();
            let target = find_active_link_member(group, target_cid)?.clone();
            let source_region = session_link::member_variant(group, &source).region();
            let target_region = session_link::member_variant(group, &target).region();
            if source_region == target_region && source.uid == target.uid {
                return Err("请为同步选择不同账号的会话副本".to_string());
            }
            if preview {
                let active: Vec<_> = group
                    .members
                    .iter()
                    .filter(|member| member.state == MemberState::Active)
                    .map(|member| (member.uid.clone(), member.session_id.clone()))
                    .collect();
                if item["previewToken"].as_str() != Some(group_fingerprint(&active).as_str()) {
                    return Err("会话关联已变更，请刷新后重试".to_string());
                }
                if unify && source_region == region && target_region == region {
                    restore_legacy_pair_baseline(
                        &paths,
                        group,
                        &source,
                        &target,
                        &legacy_baselines,
                    )?;
                }
                let details = linked_workbuddy::preview_members(&paths, group, &source, &target);
                if details["verdict"] == SyncVerdict::Identical.as_str() {
                    result["unchanged"] = json!(true);
                    return Ok(());
                }
                let available = details["availableModes"].as_array();
                let fast_forward =
                    available.is_some_and(|modes| modes.iter().any(|mode| mode == "fastForward"));
                let mode = if fast_forward {
                    SyncMode::FastForward
                } else if unify
                    && available.is_some_and(|modes| modes.iter().any(|mode| mode == "overwrite"))
                {
                    SyncMode::Overwrite
                } else if unify && details["verdict"] == SyncVerdict::Ahead.as_str() {
                    SyncMode::UnifyOverwrite
                } else {
                    return Err(details["reason"]
                        .as_str()
                        .unwrap_or("会话内容无法同步")
                        .to_owned());
                };
                let token = details["previewToken"].as_str().ok_or("无法保存同步预览")?;
                result["previewToken"] = json!(token);
                result["mode"] = json!(mode.as_str());
                result["lostLines"] = json!(if mode == SyncMode::FastForward {
                    0
                } else {
                    details["extraB"].as_u64().unwrap_or_default()
                });
                return Ok(());
            }

            let token_id = item["previewToken"].as_str().ok_or("请先预览同步")?;
            let token = session_link::load_preview_token(&paths, token_id)
                .ok_or("同步预览已失效，请重新预览")?;
            if token.binding.group_id != group.id
                || token.binding.source.member_id != source.member_id
                || token.binding.target.member_id != target.member_id
            {
                return Err("同步预览与所选会话不匹配".to_string());
            }
            let mode = SyncMode::parse(item["mode"].as_str().unwrap_or_default())?;
            if mode != SyncMode::FastForward
                && !(unify && matches!(mode, SyncMode::Overwrite | SyncMode::UnifyOverwrite))
            {
                return Err("该同步入口不支持此操作".to_string());
            }
            let source_account = json!({"uid": source.uid, "id": source.account_id, "region": source_region.as_str()});
            let target_account = json!({"uid": target.uid, "id": target.account_id, "region": target_region.as_str()});
            let selection = linked_workbuddy::SyncSelection {
                group_id: group.id.clone(),
                preview_token: token_id.to_owned(),
                mode,
            };
            let report = linked_workbuddy::sync_sessions_cross(
                &source_account,
                &target_account,
                &[selection],
            )?;
            let synced = &report["synced"][0];
            if synced.is_null() {
                let error = report["errors"][0]["error"]
                    .as_str()
                    .or_else(|| report["skipped"][0]["message"].as_str())
                    .unwrap_or("会话同步未完成");
                return Err(error.to_owned());
            }
            let before = synced["recordCount"]["targetBefore"]
                .as_u64()
                .unwrap_or_default();
            let after = synced["recordCount"]["target"].as_u64().unwrap_or_default();
            result["appendedLines"] = json!(after.saturating_sub(before));
            result["overwritten"] = json!(mode != SyncMode::FastForward);
            result["previousLines"] = json!(before);
            result["totalLines"] = json!(after);
            result["lostLines"] = synced["lostLines"].clone();
            result["backup"] = synced["backup"].clone();
            Ok(())
        })();
        match outcome {
            Ok(()) => {
                result["ok"] = json!(true);
                ok_count += 1;
            }
            Err(error) => result["error"] = json!(error),
        }
        results.push(result);
    }
    Ok(json!({"results": results, "okCount": ok_count, "failCount": items.len() - ok_count}))
}

/// Compatibility adapter for the desktop and WebUI copy report.
pub fn copy_linked_sessions_for_switch(
    source_region: Region,
    target_region: Region,
    target_acc: &Value,
    session_ids: &[String],
) -> Result<Value, String> {
    if session_ids.is_empty() {
        return Ok(json!({"copied": []}));
    }
    let (source, target) = linked_switch_accounts(source_region, target_region, target_acc)?;
    let mut report = linked_workbuddy::copy_sessions_cross(&source, &target, session_ids)?;
    report["skipped"] = json!(report["alreadyLinked"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| { json!({"id": item["id"], "newId": item["sessionId"], "deduplicated": true}) })
        .collect::<Vec<_>>());
    if let Some(copied) = report["copied"].as_array_mut() {
        for item in copied {
            item["jsonlCopied"] = json!(true);
            item["sessionRowWritten"] = json!(true);
            item["ledgerWritten"] = json!(true);
        }
    }
    Ok(report)
}

fn linked_switch_accounts(
    source_region: Region,
    target_region: Region,
    target: &Value,
) -> Result<(Value, Value), String> {
    ensure_link_store()?;
    let source_uid = current_user_uid_for(source_region).ok_or("找不到来源账号的登录状态")?;
    let mut source = account::find_account_for(source_region, &source_uid)
        .unwrap_or_else(|| json!({"uid": source_uid}));
    source["region"] = json!(source_region.as_str());
    let mut target = target.clone();
    target["region"] = json!(target_region.as_str());
    Ok((source, target))
}

pub fn preview_linked_switch(
    source_region: Region,
    target_region: Region,
    account_id: &str,
) -> Result<Value, String> {
    let target = account::find_account_for(target_region, account_id).ok_or("目标账号不存在")?;
    let (source, target) = linked_switch_accounts(source_region, target_region, &target)?;
    if source_region == target_region {
        let legacy = load_session_baselines_at(&session_baseline_path_for(source_region));
        if !legacy.is_empty() {
            let paths = SessionPaths::for_variant(WbVariant::for_region(source_region));
            if let StoreState::Ready(mut store) = session_link::load_store(&paths) {
                let variant = WbVariant::for_region(source_region);
                for group in &mut store.groups {
                    let Some(from) = session_link::active_member_for_variant(
                        group,
                        variant,
                        source["uid"].as_str().unwrap_or_default(),
                    )
                    .cloned() else {
                        continue;
                    };
                    let Some(to) = session_link::active_member_for_variant(
                        group,
                        variant,
                        target["uid"].as_str().unwrap_or_default(),
                    )
                    .cloned() else {
                        continue;
                    };
                    if from.member_id != to.member_id {
                        restore_legacy_pair_baseline(&paths, group, &from, &to, &legacy)?;
                    }
                }
            }
        }
    }
    linked_workbuddy::session_links_preview_cross(&source, &target)
}

pub fn sync_linked_sessions_for_switch(
    source_region: Region,
    target_region: Region,
    target: &Value,
    selections: &[linked_workbuddy::SyncSelection],
) -> Result<Value, String> {
    let (source, target) = linked_switch_accounts(source_region, target_region, target)?;
    linked_workbuddy::sync_sessions_cross(&source, &target, selections)
}
