mod common;

use claudehud::settings::{load, save, settings_path, Edge, Settings, FILE_NAME};

#[test]
fn missing_and_corrupt_files_give_defaults() {
    let t = common::TempDir::new("settings");
    assert_eq!(load(&t.path().join("nope.json")), Settings::default());
    let p = t.write("bad.json", "{not json");
    assert_eq!(load(&p), Settings::default());
    let bom = t.write("bom.json", "\u{feff}{\"edge\":\"left\"}");
    assert_eq!(load(&bom).edge, Edge::Left);
}

#[test]
fn save_then_load_round_trips() {
    let t = common::TempDir::new("settings-rt");
    let p = t.path().join("sub").join(FILE_NAME);
    let s = Settings {
        edge: Edge::Left,
        warn_percent: 90,
        autostart: true,
        ..Default::default()
    };
    save(&p, &s).unwrap();
    assert_eq!(load(&p), s);
    save(&p, &Settings::default()).unwrap(); // overwrite existing
    assert_eq!(load(&p), Settings::default());
}

#[test]
fn path_prefers_exe_dir_and_falls_back_to_appdata() {
    let t = common::TempDir::new("settings-path");
    assert_eq!(settings_path(t.path(), None), t.path().join(FILE_NAME));
    let missing = t.path().join("does-not-exist");
    let appdata = t.path().join("appdata");
    assert_eq!(
        settings_path(&missing, Some(&appdata)),
        appdata.join("ClaudeHUD").join(FILE_NAME)
    );
}
