use std::cell::Cell;

use eframe::egui::accesskit::Role;
use egui_kittest::{Harness, kittest::{NodeT, Queryable}};

use super::*;

const MASTER: rune_core::Key = [7; 32];

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rune-desktop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn app_with_keys(sites_path: PathBuf) -> App {
    App::start(sites_path, Ok(Some(Keys { master: MASTER })))
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
        length: DEFAULT_LENGTH,
        classes: "luds".into(),
        symbols: DEFAULT_SYMBOLS.into(),
        apps: vec![],
        note: String::new(),
        sealed: String::new(),
        modified: 0,
    }
}

fn type_into(harness: &mut Harness<'_, App>, nth: usize, text: &str) {
    harness.query_all_by_role(Role::TextInput).nth(nth).unwrap().focus();
    harness.run();
    harness.query_all_by_role(Role::TextInput).nth(nth).unwrap().type_text(text);
    harness.run();
}

fn click(harness: &mut Harness<'_, App>, label: &str) {
    harness.get_by_label_contains(label).scroll_to_me();
    harness.run();
    harness.get_by_label_contains(label).click();
    harness.run();
}

/// For a label that is also a heading, like the editor's "Add site".
fn button<'a>(harness: &'a Harness<'_, App>, label: &str) -> egui_kittest::Node<'a> {
    harness
        .query_all_by_role(Role::Button)
        .find(|b| b.accesskit_node().label().is_some_and(|l| l.contains(label)))
        .unwrap()
}

fn click_button(harness: &mut Harness<'_, App>, label: &str) {
    button(harness, label).scroll_to_me();
    harness.run();
    button(harness, label).click();
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
    click_button(&mut harness, "Add site");

    // Saving stamps the change, so it wins over older copies when devices sync.
    let modified = harness.state().sites.sites[0].modified;
    assert!(modified > 0);
    let expected = Site { modified, ..site("github.com", "bassam") };
    assert_eq!(harness.state().sites.sites, vec![expected.clone()]);
    assert_eq!(SiteList::load(&path).unwrap().sites, vec![expected]);
    assert_eq!(harness.state().selected, Some(0));

    let pw = password(&site_key(&MASTER, "github.com", "bassam", 1), DEFAULT_LENGTH, "luds", DEFAULT_SYMBOLS).unwrap();
    assert!(harness.query_by_label(&pw).is_none(), "password must be hidden until shown");
    click(&mut harness, "Show password");
    harness.get_by_label(&pw);
    assert!(harness.query_by_label("Copy previous password").is_none(), "counter 1 has no previous");

    click(&mut harness, "Add site");
    type_into(&mut harness, 0, "github.com");
    type_into(&mut harness, 1, "BASSAM");
    click_button(&mut harness, "Add site");
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
    let file = dir.join("out/rune-sites.json");
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
    editor.symbols = true;
    editor.symbol_set = "._-".into();
    assert_eq!(editor.to_site().unwrap().symbols, "-_.");
    editor.symbol_set = "".into();
    assert!(editor.to_site().is_err());
    editor.symbol_set = "é".into();
    assert!(editor.to_site().is_err());
    editor.symbols = false;
    editor.site = "   ".into();
    assert!(editor.to_site().is_err());
}

#[test]
fn setup_checks_the_typed_back_words() {
    let mut app = App::start(temp_dir("setup").join("sites.json"), Ok(None));
    app.generated = Some("velvet orbit candle mosaic frosty lantern".into());
    let mut harness = harness(app, Harness::builder());
    type_into(&mut harness, 0, "velvet orbit candle");
    click(&mut harness, "Continue");
    assert!(matches!(harness.state().screen, Screen::Setup));
    assert_eq!(harness.state().error, "That doesn't match the words above.");

    // Case and extra spaces don't matter when typing it back.
    type_into(&mut harness, 0, " Mosaic frosty  lantern");
    harness.get_by_label_contains("Continue").click();
    harness.step(); // the next screen spins while Argon2 runs, so it never settles
    assert!(matches!(harness.state().screen, Screen::Deriving { .. }));
}

#[test]
fn own_master_password_is_hidden_until_shown() {
    let mut app = App::start(temp_dir("reveal").join("sites.json"), Ok(None));
    app.generated = None;
    let mut harness = harness(app, Harness::builder());
    // Masked fields are password inputs to assistive tech; revealed ones are plain text inputs.
    assert_eq!(harness.query_all_by_role(Role::PasswordInput).count(), 2);

    harness.query_all_by_label("Show password").next().unwrap().click();
    harness.run();
    assert_eq!(harness.query_all_by_role(Role::PasswordInput).count(), 0, "one switch reveals both");
}

#[test]
fn an_old_password_is_kept_sealed_until_switched() {
    let path = temp_dir("keep").join("sites.json");
    let mut app = app_with_keys(path.clone());
    app.sites.sites = vec![site("github.com", "")];
    app.selected = Some(0);
    let mut harness = harness(app, Harness::builder());

    harness.get_by_label_contains("Use my old password for now").click();
    harness.run();
    harness.get_by_role(Role::PasswordInput).click();
    harness.run();
    harness.get_by_role(Role::PasswordInput).type_text("Hunter2!");
    harness.run();
    harness.get_by_label_contains("Keep it").click();
    harness.run();

    let sealed = harness.state().sites.sites[0].sealed.clone();
    assert!(!sealed.is_empty() && !sealed.contains("Hunter2"));
    assert_eq!(rune_core::open(&rune_core::seal_key(&MASTER), &sealed).unwrap(), "Hunter2!");
    assert_eq!(SiteList::load(&path).unwrap().sites[0].sealed, sealed, "saved, not just shown");

    harness.get_by_label_contains("Switch to Rune's password").click();
    harness.run();
    harness.get_by_label("Yes, I've changed it").click();
    harness.run();
    assert_eq!(harness.state().sites.sites[0].sealed, "");
}

#[test]
fn a_phone_with_the_same_master_syncs_over_the_network() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream, UdpSocket};

    let path = temp_dir("sync").join("sites.json");
    SiteList { version: 1, sites: vec![site("a", ""), site("b", "")], deleted: vec![] }.save(&path).unwrap();
    let key = rune_core::sync_key(&MASTER);
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let listener = store::listen_for_sync(port, key, "ef15", path.clone(), || {}).unwrap();

    // Found by its own fingerprint only.
    let phone = UdpSocket::bind("127.0.0.1:0").unwrap();
    phone.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    let mut answer = [0u8; 64];
    phone.send_to(b"rune-sync/1 0000", ("127.0.0.1", port)).unwrap();
    assert!(phone.recv_from(&mut answer).is_err());
    phone.send_to(b"rune-sync/1 ef15", ("127.0.0.1", port)).unwrap();
    let (n, _) = phone.recv_from(&mut answer).unwrap();
    assert_eq!(&answer[..n], b"rune-sync/1 ef15");

    // The phone deleted "a" and added "c".
    let mut theirs = SiteList { version: 1, sites: vec![site("b", ""), site("c", "")], deleted: vec![] };
    theirs.deleted.push(rune_core::Deleted { site: "a".into(), login: String::new(), modified: 5 });
    let exchange = |key: &rune_core::Key| {
        let message = rune_core::sync_seal(key, &theirs, [3; 12]);
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream.write_all(&(message.len() as u32).to_be_bytes()).unwrap();
        stream.write_all(&message).unwrap();
        let mut length = [0u8; 4];
        stream.read_exact(&mut length).ok()?;
        let mut reply = vec![0u8; u32::from_be_bytes(length) as usize];
        stream.read_exact(&mut reply).ok()?;
        rune_core::sync_open(key, &reply).ok()
    };

    assert!(exchange(&rune_core::sync_key(&[8; 32])).is_none(), "another master password gets no answer");
    assert_eq!(SiteList::load(&path).unwrap().sites.len(), 2, "and changes nothing");

    let merged = exchange(&key).unwrap();
    assert_eq!(merged.sites.iter().map(|s| s.site.as_str()).collect::<Vec<_>>(), ["b", "c"]);
    assert_eq!(SiteList::load(&path).unwrap(), merged, "saved, not just answered");
    assert_eq!(listener.synced.recv_timeout(Duration::from_secs(1)), Ok(true));

    assert_eq!(exchange(&key).unwrap(), merged);
    assert_eq!(listener.synced.recv_timeout(Duration::from_secs(1)), Ok(false), "a second sync changes nothing");
}

#[test]
fn strength_ranks_common_passwords_weak_and_passphrases_strong() {
    assert_eq!(strength("Bassam2024!").1, theme::DANGER);
    assert_eq!(strength("password123").1, theme::DANGER);
    assert_eq!(strength("velvet orbit candle mosaic frosty lantern").1, theme::ACCENT);
}

/// Renders every screen to target/screenshots/ for a visual check:
/// `cargo test -p rune-desktop -- --ignored screenshots`
#[test]
#[ignore = "needs a GPU adapter; writes PNGs"]
fn screenshots() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots");
    std::fs::create_dir_all(&out).unwrap();
    let dir = temp_dir("shots");
    let sites = vec![
        site("bank.example", ""),
        Site {
            counter: 2,
            note: "username: bassam_m".into(),
            apps: vec!["com.github.android".into()],
            ..site("github.com", "bassam")
        },
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
        app.generated = Some("velvet orbit candle mosaic frosty lantern".into());
        app.focus_search = false;
    });
    shoot("2-confirm", &|app| {
        app.screen = Screen::Confirm { keys: Keys { master: MASTER } };
    });
    shoot("3-empty", &|_| {});
    shoot("4-sites", &|app| {
        app.sites.sites = sites.clone();
        app.selected = Some(1);
        app.show_password = true;
    });
    shoot("4b-kept", &|app| {
        app.sites.sites = sites.clone();
        app.sites.sites[1].sealed = rune_core::seal(&rune_core::seal_key(&MASTER), "Hunter2!", [0; 12]);
        app.selected = Some(1);
        app.show_password = true;
    });
    shoot("5-editor", &|app| {
        app.sites.sites = sites.clone();
        app.screen = Screen::Editor(Editor::new(Some(1), &sites[1]));
    });
    shoot("5b-add", &|app| {
        app.sites.sites = sites.clone();
        app.screen = Screen::Editor(Editor { focus_site: false, ..Editor::blank() });
    });
    shoot("6-settings", &|app| {
        app.sites.sites = sites.clone();
        app.screen = Screen::Settings(Settings { message: "Imported: 2 new, 1 updated.".into(), ..Default::default() });
    });
    shoot("8-setup-own", &|app| {
        app.keys = None;
        app.screen = Screen::Setup;
        app.generated = None;
        app.password = "Bassam2024!".into();
        app.error = "The two passwords don't match.".into();
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

/// Needs the internet: `cargo test -p rune-desktop -- --ignored favicon`.
#[test]
#[ignore]
fn favicon() {
    // Safety: ignored tests run on their own here; nothing else reads the environment meanwhile.
    unsafe { std::env::set_var("XDG_CACHE_HOME", temp_dir("favicon")) };
    assert_eq!(store::favicon("my bank", || {}).map(|i| i.is_some()), Some(false));

    let (tx, rx) = mpsc::channel();
    assert!(store::favicon("github.com", move || tx.send(()).unwrap()).is_none());
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let icon = store::favicon("github.com", || {}).unwrap().unwrap();
    assert!(icon.size[0] > 0 && icon.size[0] <= 72);
}

#[test]
fn tile_color() {
    let clear = egui::Color32::TRANSPARENT;
    // A 4×4 logo: `edge` all round, `middle` in the 2×2 centre.
    let logo = |edge: egui::Color32, middle: egui::Color32| egui::ColorImage::new(
        [4, 4],
        (0..16).map(|i| if (1..=2).contains(&(i % 4)) && (1..=2).contains(&(i / 4)) { middle } else { edge }).collect(),
    );

    assert_eq!(theme::tile_color(&logo(egui::Color32::WHITE, egui::Color32::BLACK)), egui::Color32::WHITE);
    assert_eq!(theme::tile_color(&logo(clear, egui::Color32::from_rgb(0x18, 0x17, 0x17))), theme::TEXT);
    assert_eq!(
        theme::tile_color(&logo(clear, egui::Color32::from_rgb(0x33, 0xCC, 0x66))),
        egui::Color32::from_rgba_unmultiplied(0x33, 0xCC, 0x66, 0x29)
    );
    assert_eq!(theme::tile_color(&logo(clear, clear)), theme::TEXT);
}

#[test]
fn site_title() {
    use store::site_title;
    assert_eq!(
        site_title(r#"<title>GitHub · Build and ship software</title><meta property="og:site_name" content="GitHub">"#).as_deref(),
        Some("GitHub")
    );
    assert_eq!(site_title(r#"<META content="Mom's Shop" name="application-name" />"#).as_deref(), Some("Mom's Shop"));
    assert_eq!(site_title("<title>\n  Stack Overflow - Where Developers Learn\n</title>").as_deref(), Some("Stack Overflow"));
    assert_eq!(site_title("<title>AT&amp;T | Official Site</title>").as_deref(), Some("AT&T"));
    assert_eq!(site_title("<title>Amazon.com. Spend less. Smile more.</title>").as_deref(), Some("Amazon.com"));
    assert_eq!(site_title("<title>Caf&#233; &eacute;t&eacute; ☕</title>").as_deref(), Some("Café &eacute;t&eacute; ☕"));
    assert_eq!(site_title("<html><body>hi</body></html>"), None);
    assert_eq!(site_title("<title>Just a moment...</title>"), None);
    assert_eq!(site_title(&format!("<title>{}</title>", "x".repeat(60))), None);
    assert_eq!(store::title_case("my bank"), "My Bank");
    assert_eq!(store::title_case("github.com"), "github.com");
}
