use std::cell::Cell;

use eframe::egui::accesskit::Role;
use egui_kittest::{Harness, kittest::Queryable};

use super::*;

const MASTER: echo_core::Key = [7; 32];

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("echo-desktop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn app_with_keys(sites_path: PathBuf) -> App {
    App::start(sites_path, Ok(Some(Keys { secret: [0; 32], master: MASTER })))
}

fn harness(app: App, builder: egui_kittest::HarnessBuilder<App>) -> Harness<'static, App> {
    let themed = Cell::new(false);
    let mut harness = builder.with_size([460.0, 680.0]).build_ui_state(
        move |ui, app: &mut App| {
            if !themed.replace(true) {
                // New fonts only exist from the next frame, so nothing is drawn on this one.
                theme::apply(ui.ctx());
                ui.ctx().request_repaint();
                return;
            }
            app.draw(ui);
        },
        app,
    );
    harness.run();
    harness
}

fn site(name: &str, login: &str) -> Site {
    Site {
        site: name.into(),
        login: login.into(),
        counter: 1,
        length: 20,
        classes: "luds".into(),
        apps: vec![],
    }
}

fn type_into(harness: &mut Harness<'_, App>, nth: usize, text: &str) {
    harness.query_all_by_role(Role::TextInput).nth(nth).unwrap().focus();
    harness.run();
    harness.query_all_by_role(Role::TextInput).nth(nth).unwrap().type_text(text);
    harness.run();
}

fn click(harness: &mut Harness<'_, App>, label: &str) {
    harness.get_by_label_contains(label).click();
    harness.run();
}

/// Drives the real UI headlessly: add a site, read its password, hit the duplicate check, delete it.
#[test]
fn add_show_and_delete_a_site() {
    let dir = temp_dir("flow");
    let path = dir.join("sites.json");
    let mut harness = harness(app_with_keys(path.clone()), Harness::builder());
    harness.get_by_label("No sites yet");

    click(&mut harness, "Add a site");
    type_into(&mut harness, 0, "  GitHub.COM");
    type_into(&mut harness, 1, "Bassam ");
    click(&mut harness, "Save");

    let expected = site("github.com", "bassam");
    assert_eq!(harness.state().sites.sites, vec![expected.clone()]);
    assert_eq!(SiteList::load(&path).unwrap().sites, vec![expected]);
    assert_eq!(harness.state().selected, Some(0));

    let pw = password(&site_key(&MASTER, "github.com", "bassam", 1), 20, "luds").unwrap();
    assert!(harness.query_by_label(&pw).is_none(), "password must be hidden until shown");
    click(&mut harness, "Show password");
    harness.get_by_label(&pw);

    click(&mut harness, "Add site");
    type_into(&mut harness, 0, "github.com");
    type_into(&mut harness, 1, "BASSAM");
    click(&mut harness, "Save");
    harness.get_by_label_contains("That site and login already exist.");
    assert_eq!(harness.state().sites.sites.len(), 1);
    click(&mut harness, "Cancel");
    assert!(harness.state().error.is_empty(), "leaving the editor clears its error");

    click(&mut harness, "github.com, bassam");
    click(&mut harness, "Edit site");
    click(&mut harness, "Delete");
    click(&mut harness, "Yes, delete it");
    assert!(harness.state().sites.sites.is_empty());
    assert!(SiteList::load(&path).unwrap().sites.is_empty());
    harness.get_by_label("No sites yet");

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn search_filters_the_list() {
    let dir = temp_dir("search");
    let mut app = app_with_keys(dir.join("sites.json"));
    app.sites.sites = vec![site("github.com", "bassam"), site("gitlab.com", ""), site("bank.example", "")];
    let mut harness = harness(app, Harness::builder());
    type_into(&mut harness, 0, "GIT");
    harness.get_by_label("github.com, bassam");
    harness.get_by_label("gitlab.com");
    assert!(harness.query_by_label("bank.example").is_none());
    type_into(&mut harness, 0, "zzz");
    harness.get_by_label("No sites match your search.");
}

#[test]
fn export_then_import_merges() {
    let dir = temp_dir("transfer");
    let mut app = app_with_keys(dir.join("sites.json"));
    app.sites.sites = vec![site("a.example", ""), site("b.example", "")];
    let file = dir.join("out/echo-sites.json");
    app.export_sites(&file).unwrap();

    let mut other = app_with_keys(dir.join("other/sites.json"));
    let mut rotated = site("a.example", "");
    rotated.counter = 1;
    other.sites.sites = vec![rotated, site("c.example", "")];
    let message = other.import_sites(&file).unwrap();
    assert_eq!(message, "Imported: 1 new, 0 updated.");
    let names: Vec<_> = other.sites.sites.iter().map(|s| s.site.as_str()).collect();
    assert_eq!(names, ["a.example", "b.example", "c.example"]);
    assert_eq!(SiteList::load(&dir.join("other/sites.json")).unwrap(), other.sites);

    assert!(other.import_sites(&dir.join("missing-is-empty.json")).is_ok());
    std::fs::write(dir.join("bad.json"), "nope").unwrap();
    assert!(other.import_sites(&dir.join("bad.json")).is_err());
    assert_eq!(other.sites.sites.len(), 3, "a failed import changes nothing");

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn editor_normalises_and_validates() {
    let mut editor = Editor::new(None, &site("  GitHub.COM ", " Bassam"));
    editor.upper = false;
    editor.symbols = false;
    let site = editor.to_site().unwrap();
    assert_eq!(
        (site.site.as_str(), site.login.as_str(), site.classes.as_str()),
        ("github.com", "bassam", "ld")
    );
    editor.lower = false;
    editor.digits = false;
    assert!(editor.to_site().is_err());
    editor.digits = true;
    editor.site = "   ".into();
    assert!(editor.to_site().is_err());
}

/// Renders every screen to target/screenshots/ for a visual check:
/// `cargo test -p echo-desktop -- --ignored screenshots`
#[test]
#[ignore = "needs a GPU adapter; writes PNGs"]
fn screenshots() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots");
    std::fs::create_dir_all(&out).unwrap();
    let dir = temp_dir("shots");
    let sites = vec![
        site("bank.example", ""),
        site("github.com", "bassam"),
        site("github.com", "work"),
        site("mail.example", "bassam@example.com"),
        site("news.example", ""),
        site("shop.example", "bassam"),
    ];
    let shoot = |name: &str, prepare: &dyn Fn(&mut App)| {
        let mut app = app_with_keys(dir.join("sites.json"));
        prepare(&mut app);
        let mut harness = harness(app, Harness::builder().with_pixels_per_point(1.5).wgpu());
        harness.run();
        harness.render().unwrap().save(out.join(format!("{name}.png"))).unwrap();
    };

    shoot("1-setup", &|app| {
        app.keys = None;
        app.screen = Screen::Setup;
        app.focus_search = false;
    });
    shoot("2-confirm", &|app| {
        app.screen = Screen::Confirm {
            keys: Keys { secret: core::array::from_fn(|i| i as u8 * 7), master: MASTER },
            generated: true,
            written_down: false,
        };
    });
    shoot("3-empty", &|_| {});
    shoot("4-sites", &|app| {
        app.sites.sites = sites.clone();
        app.selected = Some(1);
        app.show_password = true;
    });
    shoot("5-editor", &|app| {
        app.sites.sites = sites.clone();
        app.screen = Screen::Editor(Editor::new(Some(1), &sites[1]));
    });
    shoot("6-settings", &|app| {
        app.sites.sites = sites.clone();
        app.screen = Screen::Settings(Settings { message: "Imported: 2 new, 1 updated.".into(), ..Default::default() });
    });
    shoot("8-setup-with-key", &|app| {
        app.keys = None;
        app.screen = Screen::Setup;
        app.have_secret_key = true;
        app.error = "That isn't a valid secret key.".into();
    });
    shoot("7-error", &|app| {
        app.sites.sites = sites.clone();
        let mut editor = Editor::blank();
        editor.symbols = false;
        editor.length = 64;
        app.screen = Screen::Editor(editor);
        app.error = "Enter a site name.".into();
    });
}
