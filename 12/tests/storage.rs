mod common;

use chrono::Utc;
use lab12::domain::memory::{
    ChatMessage, LtmState, MemoryBundle, MemoryLayer, MemoryMetaState, WorkingMemoryState,
};
use lab12::storage::paths::DataPaths;
use lab12::storage::{
    acquire_lock, load_bundle, load_ltm, load_pending, load_profiles, load_settings, load_stm,
    load_working, save_bundle, save_ltm, save_pending, save_profiles, save_settings, save_stm,
    save_working,
};

#[test]
fn every_layer_file_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::new(dir.path());
    paths.ensure().unwrap();
    let now = Utc::now();
    let mut stm = lab12::domain::memory::StmState::empty(now);
    stm.messages.push(ChatMessage::user("привет"));
    let mut wm = WorkingMemoryState::empty(now);
    wm.task_title = "Смета".into();
    let ltm = LtmState::empty(now);
    save_stm(&paths, &stm).unwrap();
    save_working(&paths, &wm).unwrap();
    save_ltm(&paths, &ltm).unwrap();
    save_pending(&paths, &[]).unwrap();

    assert_eq!(load_stm(&paths).unwrap().messages.len(), 1);
    assert_eq!(load_working(&paths).unwrap().task_title, "Смета");
    assert!(load_ltm(&paths).unwrap().records.is_empty());
    assert!(load_pending(&paths).unwrap().is_empty());
}

#[test]
fn atomic_write_leaves_backup() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::new(dir.path());
    let now = Utc::now();
    let mut wm = WorkingMemoryState::empty(now);
    wm.task_title = "первая".into();
    save_working(&paths, &wm).unwrap();
    wm.task_title = "вторая".into();
    save_working(&paths, &wm).unwrap();
    let backup = std::fs::read_to_string(paths.working().with_extension("json.bak")).unwrap();
    assert!(backup.contains("первая"));
}

#[test]
fn corrupt_layer_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::new(dir.path());
    paths.ensure().unwrap();
    std::fs::write(paths.ltm(), "{broken").unwrap();
    assert!(load_ltm(&paths).is_err());
    assert_eq!(std::fs::read(paths.ltm()).unwrap(), b"{broken");
}

#[test]
fn lock_blocks_second_instance() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::new(dir.path());
    let _first = acquire_lock(&paths).unwrap();
    assert!(acquire_lock(&paths).is_err());
}

#[cfg(unix)]
#[test]
fn settings_permissions_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::new(dir.path());
    let settings = common::test_settings();
    save_settings(&paths, &settings).unwrap();
    assert_eq!(load_settings(&paths).unwrap(), settings);
    let mode = std::fs::metadata(paths.settings())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn bundle_save_writes_five_independent_files() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::new(dir.path());
    let bundle = MemoryBundle {
        meta: MemoryMetaState::empty(),
        ..MemoryBundle::empty(Utc::now())
    };
    save_bundle(&paths, &bundle).unwrap();
    for path in [
        paths.stm_conversation(),
        paths.stm_summary(),
        paths.working(),
        paths.ltm(),
        paths.pending(),
        paths.meta(),
    ] {
        assert!(path.exists(), "{}", path.display());
    }
    assert_eq!(
        load_bundle(&paths).unwrap().stm.conversation_id,
        bundle.stm.conversation_id
    );
}

#[test]
fn profiles_roundtrip_and_corrupt_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::new(dir.path());
    paths.ensure().unwrap();
    let state = lab12::domain::profile::ProfileState::with_presets(Utc::now());
    save_profiles(&paths, &state).unwrap();
    let loaded = load_profiles(&paths).unwrap();
    assert_eq!(loaded, state);
    assert!(paths.profiles().exists());

    std::fs::write(paths.profiles(), "{broken").unwrap();
    assert!(load_profiles(&paths).is_err());
    assert_eq!(std::fs::read(paths.profiles()).unwrap(), b"{broken");
}

#[test]
fn clearing_one_layer_leaves_others_intact() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::seeded_controller(dir.path());
    assert!(!controller.bundle().stm.messages.is_empty());
    assert!(!controller.bundle().wm.items.is_empty());
    assert!(!controller.bundle().ltm.records.is_empty());

    controller.clear_layer(MemoryLayer::Ltm).unwrap();
    assert!(controller.bundle().ltm.records.is_empty());
    assert!(controller.bundle().pending.is_empty());
    assert!(!controller.bundle().stm.messages.is_empty());
    assert!(!controller.bundle().wm.items.is_empty());

    let reloaded = common::controller_at(dir.path());
    assert!(reloaded.bundle().ltm.records.is_empty());
    assert!(!reloaded.bundle().wm.items.is_empty());
    assert!(!reloaded.bundle().stm.messages.is_empty());
}
