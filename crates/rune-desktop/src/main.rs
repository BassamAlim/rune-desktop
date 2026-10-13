mod bitwarden;
mod store;
mod theme;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use arboard::Clipboard;
use rune_core::{
    DEFAULT_LENGTH, DEFAULT_SYMBOLS, Site, SiteList, canonical_symbols, entropy_bits, fingerprint, master_key, passphrase, passphrase_bits,
    normalize, open, password, seal, seal_key, site_key,
};
use eframe::egui::{self, Align, Key, Layout, RichText, Sense, Ui};
use store::Keys;
use theme::icon;
use zeroize::Zeroize;

/// Content never gets wider than this; on a wide window it is centred.
const CONTENT_WIDTH: f32 = 520.0;
const COPIED_FLASH: Duration = Duration::from_secs(2);

struct Editor {
    /// Index in the site list when editing; `None` when adding.
    index: Option<usize>,
    site: String,
    login: String,
    counter: u32,
    length: usize,
    lower: bool,
    upper: bool,
    digits: bool,
    symbols: bool,
    symbol_set: String,
    apps: Vec<String>,
    note: String,
    /// Carried through an edit untouched: only the site card sets or clears it.
    sealed: String,
    confirm_delete: bool,
    focus_site: bool,
    /// Everything but site, login and note is rarely touched, so it folds away unless in use.
    expanded: bool,
}

impl Editor {
    fn new(index: Option<usize>, site: &Site) -> Self {
        Self {
            index,
            site: site.site.clone(),
            login: site.login.clone(),
            counter: site.counter,
            length: site.length,
            lower: site.classes.contains('l'),
            upper: site.classes.contains('u'),
            digits: site.classes.contains('d'),
            symbols: site.classes.contains('s'),
            symbol_set: site.symbols.clone(),
            apps: site.apps.clone(),
            note: site.note.clone(),
            sealed: site.sealed.clone(),
            confirm_delete: false,
            focus_site: index.is_none(),
            expanded: site.length != DEFAULT_LENGTH
                || site.classes != "luds"
                || site.symbols != DEFAULT_SYMBOLS
                || site.counter != 1,
        }
    }

    fn blank() -> Self {
        Self::new(
            None,
            &Site {
                site: String::new(),
                login: String::new(),
                counter: 1,
                length: DEFAULT_LENGTH,
                classes: "luds".to_string(),
                symbols: DEFAULT_SYMBOLS.to_string(),
                apps: vec![],
                note: String::new(),
                sealed: String::new(),
                modified: 0,
            },
        )
    }

    fn classes(&self) -> String {
        [(self.lower, 'l'), (self.upper, 'u'), (self.digits, 'd'), (self.symbols, 's')]
            .iter()
            .filter_map(|&(on, class)| on.then_some(class))
            .collect()
    }

    fn to_site(&self) -> Result<Site, String> {
        let classes = self.classes();
        let site = normalize(&self.site);
        if site.is_empty() {
            return Err("Enter a site name.".to_string());
        }
        if classes.is_empty() {
            return Err("Pick at least one kind of character.".to_string());
        }
        let symbols = canonical_symbols(&self.symbol_set)
            .map_err(|_| "Symbols can only be ASCII punctuation, like ! # - .".to_string())?;
        if self.symbols && symbols.is_empty() {
            return Err("Enter the symbols this site allows, or turn symbols off.".to_string());
        }
        Ok(Site {
            site,
            login: normalize(&self.login),
            counter: self.counter,
            length: self.length,
            classes,
            symbols,
            apps: self.apps.clone(),
            // Kept as typed: a username's case can matter.
            note: self.note.trim().to_owned(),
            sealed: self.sealed.clone(),
            // Stamped by `SiteList::put` when it is saved.
            modified: 0,
        })
    }
}

#[derive(Clone, Copy)]
enum Dialog {
    Export,
    Import,
}

#[derive(Default)]
struct Settings {
    confirm_reset: bool,
    message: String,
    /// A file dialog is open on another thread.
    dialog: Option<(Dialog, Receiver<Option<PathBuf>>)>,
}

enum Screen {
    Setup,
    Deriving { rx: Receiver<Keys> },
    Confirm { keys: Keys },
    Sites,
    Editor(Editor),
    Settings(Settings),
    /// Something is wrong that the app must not paper over (e.g. an unreadable site list).
    Fatal(String),
}

struct App {
    screen: Screen,
    error: String,
    mark: Option<egui::TextureHandle>,

    // Setup form. `generated` holds the suggested passphrase; `None` means "use my own".
    generated: Option<String>,
    password: String,
    confirm: String,
    show_master: bool,

    // Set once setup is done
    keys: Option<Keys>,
    sites: SiteList,
    sites_path: PathBuf,

    // Sites screen
    search: String,
    focus_search: bool,
    selected: Option<usize>,
    show_password: bool,
    /// The site's own password being typed in, to keep until it's changed to Rune's.
    keeping: Option<String>,
    /// Asked once before the kept password is thrown away.
    confirm_switch: bool,
    clipboard: Option<Clipboard>,
    copied_at: Option<Instant>,
    /// Favicons and their tile colours by site name, once settled; `None` for a site that has none.
    icons: HashMap<String, Option<(egui::TextureHandle, egui::Color32)>>,
    /// Names sites give themselves ("GitHub"), once settled; `None` for a site that gives none.
    names: HashMap<String, Option<String>>,
    /// Off in tests, so rendering a screen never touches the network.
    online: bool,
    /// Answers the phone's sync (SPEC §3.3) while there are keys to prove it's ours.
    sync: Option<store::SyncListener>,
    /// Why sync isn't listening, e.g. another Rune already is.
    sync_error: Option<String>,
    synced_at: Option<Instant>,
    /// A sync changed the file while the editor was open; reloaded on leaving it.
    reload_sites: bool,
}

impl App {
    fn new() -> Self {
        Self { online: true, ..Self::start(store::sites_path(), store::load_keys(store::ACCOUNT)) }
    }

    fn start(sites_path: PathBuf, keys: Result<Option<Keys>, String>) -> Self {
        let mut app = Self {
            screen: Screen::Setup,
            error: String::new(),
            mark: None,
            generated: Some(new_passphrase()),
            password: String::new(),
            confirm: String::new(),
            show_master: false,
            keys: None,
            sites: SiteList { version: 1, sites: vec![], deleted: vec![] },
            sites_path,
            search: String::new(),
            focus_search: true,
            selected: None,
            icons: HashMap::new(),
            names: HashMap::new(),
            online: false,
            sync: None,
            sync_error: None,
            synced_at: None,
            reload_sites: false,
            show_password: false,
            keeping: None,
            confirm_switch: false,
            clipboard: None,
            copied_at: None,
        };
        match SiteList::load(&app.sites_path) {
            Ok(sites) => app.sites = sites,
            Err(e) => {
                app.screen = Screen::Fatal(e);
                return app;
            }
        }
        match keys {
            Ok(Some(keys)) => {
                app.keys = Some(keys);
                app.screen = Screen::Sites;
            }
            Ok(None) => {}
            Err(e) => app.error = format!("Can't reach the system keyring: {e}"),
        }
        app
    }

    /// The mark at `size` points. Mipmapped, so one raster stays smooth down to app-bar size.
    fn mark(&mut self, ui: &Ui, size: f32) -> egui::Image<'static> {
        let texture = self.mark.get_or_insert_with(|| {
            let bytes = include_bytes!("../../../assets/mark-192.rgba");
            let image = egui::ColorImage::from_rgba_unmultiplied([192, 192], bytes);
            let options = egui::TextureOptions::LINEAR
                .with_mipmap_mode(Some(egui::TextureFilter::Linear));
            ui.ctx().load_texture("mark", image, options)
        });
        egui::Image::new((texture.id(), egui::Vec2::splat(size)))
    }

    fn go_to_sites(&mut self) -> Screen {
        self.error.clear();
        self.focus_search = true;
        Screen::Sites
    }

    // ---- Setup -------------------------------------------------------------------------------

    fn setup_screen(&mut self, ui: &mut Ui) -> Option<Screen> {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.add(self.mark(ui, 64.0));
            ui.add_space(4.0);
            ui.heading("Set up Rune");
            ui.label(theme::muted(
                "One master password becomes a different password for every site. Nothing is stored.",
            ));
        });
        ui.add_space(16.0);

        ui.horizontal(|ui| {
            if ui.selectable_label(self.generated.is_some(), "Generate one").clicked() && self.generated.is_none() {
                self.generated = Some(new_passphrase());
                self.password.zeroize();
                self.confirm.zeroize();
            }
            if ui.selectable_label(self.generated.is_none(), "Use my own").clicked() {
                if let Some(mut phrase) = self.generated.take() {
                    phrase.zeroize();
                }
                self.confirm.zeroize();
            }
        });
        ui.add_space(4.0);

        if let Some(phrase) = &mut self.generated {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(theme::small("Your master password"));
                ui.add(egui::Label::new(RichText::new(phrase.as_str()).monospace().size(18.0).color(theme::ACCENT)).wrap());
                if ui.add(theme::button(icon::ARROWS_CLOCKWISE, "New words")).clicked() {
                    phrase.zeroize();
                    *phrase = new_passphrase();
                }
                ui.label(theme::small(
                    "Write it down until you know it by heart. Forgetting it loses every password.",
                ));
            });
            theme::field(ui, "Type it back", |ui| ui.add(theme::text_field(&mut self.confirm)));
            ui.label(theme::small(format!(
                "{:.0} bits: out of reach even if a site leaks one of your passwords.",
                passphrase_bits()
            )));
        } else {
            theme::field(ui, "Master password", |ui| {
                theme::password_field(ui, &mut self.password, &mut self.show_master)
            });
            theme::field(ui, "Repeat master password", |ui| {
                theme::password_field(ui, &mut self.confirm, &mut self.show_master)
            });
            if !self.password.is_empty() {
                let (text, color) = strength(&self.password);
                ui.add(egui::Label::new(RichText::new(text).size(13.0).color(color)).wrap());
            }
        }
        ui.add_space(8.0);
        let submit = ui.add_sized([ui.available_width(), 40.0], theme::primary_button("", "Continue")).clicked()
            || ui.input(|i| i.key_pressed(Key::Enter));
        if !submit {
            return None;
        }

        let words = |s: &str| s.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
        let checked = match &self.generated {
            Some(phrase) if words(phrase) == words(&self.confirm) => Ok(phrase.clone()),
            Some(_) => Err("That doesn't match the words above."),
            None if self.password.is_empty() => Err("Enter a master password."),
            None if self.password != self.confirm => Err("The two passwords don't match."),
            None => Ok(std::mem::take(&mut self.password)),
        };
        let mut password = match checked {
            Ok(password) => password,
            Err(e) => {
                self.error = e.to_string();
                return None;
            }
        };
        self.error.clear();
        self.confirm.zeroize();

        // Argon2id takes about a second; keep it off the UI thread.
        let (tx, rx) = mpsc::channel();
        let ctx = ui.ctx().clone();
        std::thread::spawn(move || {
            let master = master_key(&password);
            password.zeroize();
            let _ = tx.send(Keys { master });
            ctx.request_repaint();
        });
        Some(Screen::Deriving { rx })
    }

    fn confirm_screen(&mut self, ui: &mut Ui) -> Option<Screen> {
        let Screen::Confirm { keys } = &mut self.screen else {
            return None;
        };
        let mut next = None;
        ui.add_space(8.0);
        ui.heading("Check your fingerprint");
        ui.add_space(8.0);

        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(theme::small("Key fingerprint"));
            ui.label(RichText::new(fingerprint(&keys.master)).monospace().size(28.0).color(theme::ACCENT));
            ui.label(theme::small(
                "Every device with the same master password shows this. If you use Rune on another \
                 device and its fingerprint differs, go back and retype the master password.",
            ));
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                next = Some(Screen::Setup);
            }
            if ui.add(theme::primary_button("", "Save and continue")).clicked() {
                match store::save_keys(store::ACCOUNT, keys) {
                    Ok(()) => {
                        self.keys = Some(Keys { ..*keys });
                        if let Some(mut phrase) = self.generated.take() {
                            phrase.zeroize();
                        }
                        self.error.clear();
                        self.focus_search = true;
                        next = Some(Screen::Sites);
                    }
                    Err(e) => self.error = format!("Can't save to the system keyring: {e}"),
                }
            }
        });
        next
    }

    // ---- Sites -------------------------------------------------------------------------------

    fn copy(&mut self, text: String) {
        if self.clipboard.is_none() {
            self.clipboard = Clipboard::new().ok();
        }
        let result = match &mut self.clipboard {
            Some(clipboard) => store::copy_secret(clipboard, &text).map_err(|e| e.to_string()),
            None => Err("no clipboard available".to_string()),
        };
        match result {
            Ok(()) => {
                self.copied_at = Some(Instant::now());
                self.error.clear();
                store::clear_clipboard_later(text);
            }
            Err(e) => self.error = format!("Can't copy: {e}"),
        }
    }

    fn select(&mut self, index: Option<usize>) {
        self.selected = index;
        self.show_password = false;
        self.copied_at = None;
        if let Some(mut typed) = self.keeping.take() {
            typed.zeroize();
        }
        self.confirm_switch = false;
    }

    fn sites_screen(&mut self, ui: &mut Ui) -> Option<Screen> {
        let mut next = None;
        ui.horizontal(|ui| {
            ui.add(self.mark(ui, 30.0));
            ui.heading("rune");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::icon_button(ui, icon::GEAR, "Settings").clicked() {
                    next = Some(Screen::Settings(Settings::default()));
                }
            });
        });
        ui.add_space(4.0);

        if self.sites.sites.is_empty() {
            ui.add_space(ui.available_height() * 0.22);
            ui.vertical_centered(|ui| {
                ui.add(self.mark(ui, 72.0).tint(theme::BORDER));
                ui.add_space(8.0);
                ui.label(RichText::new("No sites yet").size(18.0).strong());
                ui.label(theme::muted("Rune works out a password for every site you add."));
                ui.add_space(12.0);
                if ui.add(theme::primary_button(icon::PLUS, "Add a site")).clicked() {
                    next = Some(Screen::Editor(Editor::blank()));
                }
            });
            return next;
        }

        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(theme::primary_button(icon::PLUS, "Add site")).clicked() {
                    next = Some(Screen::Editor(Editor::blank()));
                }
                let search = ui.add(
                    theme::text_field(&mut self.search)
                        .hint_text(theme::icon_text(icon::MAGNIFYING_GLASS, "Search sites", theme::MUTED, 15.0)),
                );
                if std::mem::take(&mut self.focus_search) {
                    search.request_focus();
                }
            });
        });
        ui.add_space(4.0);

        // The selected site's card is pinned to the bottom; the list takes whatever is left.
        if let Some(index) = self.selected.filter(|&i| i < self.sites.sites.len()) {
            egui::Panel::bottom("detail")
                .frame(egui::Frame::NONE)
                .show_separator_line(false)
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    if let Some(screen) = self.detail_card(ui, index) {
                        next = Some(screen);
                    }
                });
        }

        let query = normalize(&self.search);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let mut clicked = None;
                let mut shown = 0;
                for (i, site) in self.sites.sites.iter().enumerate() {
                    let in_note = site.note.to_lowercase().contains(&query);
                    if !site.site.contains(&query) && !site.login.contains(&query) && !in_note {
                        continue;
                    }
                    shown += 1;
                    let icon = if self.online { site_icon(&mut self.icons, ui.ctx(), &site.site) } else { None };
                    let name = display_name(&mut self.names, self.online, ui.ctx(), &site.site);
                    if site_row(ui, site, &name, icon, self.selected == Some(i)).clicked() {
                        clicked = Some(i);
                    }
                }
                if shown == 0 {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| ui.label(theme::muted("No sites match your search.")));
                }
                if clicked.is_some() {
                    self.select(clicked);
                }
            });
        });
        next
    }

    fn detail_card(&mut self, ui: &mut Ui, index: usize) -> Option<Screen> {
        let site = self.sites.sites[index].clone();
        let keys = self.keys.as_ref()?;
        let generated = password(
            &site_key(&keys.master, &site.site, &site.login, site.counter),
            site.length,
            &site.classes,
            &site.symbols,
        );
        // Changing a password on the site usually asks for the old one too.
        let mut previous = (site.counter > 1)
            .then(|| {
                let key = site_key(&keys.master, &site.site, &site.login, site.counter - 1);
                password(&key, site.length, &site.classes, &site.symbols).ok()
            })
            .flatten();
        // §3.1: a site not yet switched to Rune's password shows its own, kept sealed.
        let mut sealer = seal_key(&keys.master);
        let mut kept = (!site.sealed.is_empty()).then(|| open(&sealer, &site.sealed));
        // Some("") clears the kept password; Some(sealed) keeps a new one.
        let mut new_sealed: Option<String> = None;
        let mut next = None;
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let name = display_name(&mut self.names, self.online, ui.ctx(), &site.site);
                let subtitle = &site.login;
                ui.vertical(|ui| {
                    ui.label(RichText::new(&name).strong().size(16.0));
                    if !subtitle.is_empty() {
                        ui.label(theme::small(subtitle));
                    }
                    if !site.note.is_empty() {
                        ui.label(&site.note);
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if theme::icon_button(ui, icon::PENCIL_SIMPLE, "Edit site").clicked() {
                        next = Some(Screen::Editor(Editor::new(Some(index), &site)));
                    }
                });
            });
            let mut runes = match generated {
                Ok(pw) => pw,
                Err(e) => {
                    ui.colored_label(theme::DANGER, format!("This site's settings are invalid: {e}"));
                    return;
                }
            };
            let mut pw = match &kept {
                Some(Ok(old)) => {
                    ui.label(theme::small("Your old password. Change it on the site to Rune's, then switch."));
                    old.clone()
                }
                Some(Err(e)) => {
                    ui.colored_label(theme::DANGER, format!("The kept password can't be read: {e}"));
                    runes.clone()
                }
                None => runes.clone(),
            };

            egui::Frame::new()
                .fill(theme::BG)
                .corner_radius(theme::RADIUS)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let (glyph, label) = if self.show_password {
                                (icon::EYE_SLASH, "Hide password")
                            } else {
                                (icon::EYE, "Show password")
                            };
                            if theme::icon_button(ui, glyph, label).clicked() {
                                self.show_password = !self.show_password;
                            }
                            ui.with_layout(Layout::left_to_right(Align::Center).with_main_wrap(true), |ui| {
                                let shown = if self.show_password { pw.clone() } else { "•".repeat(pw.len()) };
                                ui.add(egui::Label::new(RichText::new(shown).monospace().size(16.0)).wrap());
                            });
                        });
                    });
                });

            let just_copied = self.copied_at.is_some_and(|at| at.elapsed() < COPIED_FLASH);
            let (glyph, label) = if just_copied { (icon::CHECK, "Copied") } else { (icon::COPY, "Copy password") };
            if ui.add_sized([ui.available_width(), 40.0], theme::primary_button(glyph, label)).clicked() {
                self.copy(pw.clone());
            }
            if just_copied {
                ui.ctx().request_repaint_after(COPIED_FLASH);
            }
            pw.zeroize();

            if kept.is_some() {
                // The site's change-password form wants the new one too.
                ui.horizontal_wrapped(|ui| {
                    if ui.add(theme::button(icon::COPY, "Copy Rune's password")).clicked() {
                        self.copy(runes.clone());
                    }
                    if !self.confirm_switch {
                        if ui.add(theme::button(icon::ARROWS_CLOCKWISE, "Switch to Rune's password")).clicked() {
                            self.confirm_switch = true;
                        }
                    } else if ui.add(theme::danger_button("", "Yes, I've changed it")).clicked() {
                        new_sealed = Some(String::new());
                    }
                });
                if self.confirm_switch {
                    ui.label(theme::small("Rune forgets the old password and shows its own from now on."));
                }
            } else if let Some(previous) = &previous
                && ui.add(theme::button(icon::CLOCK_COUNTER_CLOCKWISE, "Copy previous password")).clicked()
            {
                self.copy(previous.clone());
            }

            if kept.is_none() {
                let mut cancel = false;
                match &mut self.keeping {
                    None => {
                        if ui.add(theme::button(icon::KEY, "Use my old password for now")).clicked() {
                            self.keeping = Some(String::new());
                        }
                    }
                    Some(typed) => {
                        theme::field(ui, "Your current password on this site", |ui| {
                            ui.add(theme::text_field(typed).password(true))
                        });
                        ui.horizontal(|ui| {
                            if ui.add(theme::button(icon::CHECK, "Keep it")).clicked() && !typed.is_empty() {
                                new_sealed = Some(seal(&sealer, typed, store::nonce()));
                            }
                            cancel = ui.button("Cancel").clicked();
                        });
                    }
                }
                if cancel && let Some(mut typed) = self.keeping.take() {
                    typed.zeroize();
                }
            }
            runes.zeroize();

            ui.label(theme::small(if self.copied_at.is_some() {
                format!("The clipboard is cleared after {} seconds.", store::CLIPBOARD_CLEAR_AFTER.as_secs())
            } else {
                format!(
                    "{} characters  ·  counter {}  ·  about {:.0} bits",
                    site.length,
                    site.counter,
                    entropy_bits(site.length, &site.classes, &site.symbols)
                )
            }));
        });
        if let Some(previous) = &mut previous {
            previous.zeroize();
        }
        if let Some(Ok(old)) = &mut kept {
            old.zeroize();
        }
        sealer.zeroize();
        if let Some(sealed) = new_sealed {
            let mut updated = self.sites.clone();
            let changed = Site { sealed, ..updated.sites[index].clone() };
            match updated.put(changed, Some(index), store::now()).and_then(|()| updated.save(&self.sites_path)) {
                Ok(()) => {
                    self.sites = updated;
                    self.select(Some(index));
                }
                Err(e) => self.error = e,
            }
        }
        next
    }

    // ---- Editor ------------------------------------------------------------------------------

    fn editor_screen(&mut self, ui: &mut Ui) -> Option<Screen> {
        let Screen::Editor(editor) = &mut self.screen else {
            return None;
        };
        let mut next = None;
        let back = header(ui, if editor.index.is_some() { "Edit site" } else { "Add site" });

        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let site = theme::field(ui, "Site", |ui| {
                ui.add(theme::text_field(&mut editor.site).hint_text("github.com"))
            });
            if std::mem::take(&mut editor.focus_site) {
                site.request_focus();
            }
            // Linked on the phone, where the apps are; here they can only be unlinked.
            if !editor.apps.is_empty() {
                ui.label(theme::small("Apps linked on your phone"));
                let mut unlinked = None;
                for (i, app) in editor.apps.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::icon_button(ui, icon::X, &format!("Unlink {app}")).clicked() {
                                unlinked = Some(i);
                            }
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.add(egui::Label::new(RichText::new(app).monospace()).truncate());
                            });
                        });
                    });
                }
                if let Some(i) = unlinked {
                    editor.apps.remove(i);
                }
            }
            theme::field(ui, "Login (optional)", |ui| {
                ui.add(theme::text_field(&mut editor.login).hint_text("username or email"))
            });
            theme::field(ui, "Note (optional)", |ui| {
                ui.add(theme::text_field(&mut editor.note).hint_text("username: bassam_m"))
            });
        });

        let (glyph, label) =
            if editor.expanded { (icon::CARET_UP, "Fewer options") } else { (icon::CARET_DOWN, "More options") };
        if ui.add(egui::Button::new(theme::icon_text(glyph, label, theme::ACCENT, 15.0)).frame(false)).clicked() {
            editor.expanded = !editor.expanded;
        }

        if editor.expanded {
            theme::card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(theme::small("Length"));
                ui.add(egui::Slider::new(&mut editor.length, 4..=64).suffix(" characters"));
                ui.add_space(4.0);

                ui.label(theme::small("Characters"));
                ui.horizontal(|ui| {
                    for (on, label) in [
                        (&mut editor.lower, "a–z"),
                        (&mut editor.upper, "A–Z"),
                        (&mut editor.digits, "0–9"),
                        (&mut editor.symbols, "!@#"),
                    ] {
                        ui.toggle_value(on, label);
                    }
                });
                if editor.symbols {
                    theme::field(ui, "Symbols this site allows", |ui| {
                        ui.add(theme::text_field(&mut editor.symbol_set).font(egui::TextStyle::Monospace))
                    });
                    if ui.small_button("Reset to the usual symbols").clicked() {
                        editor.symbol_set = DEFAULT_SYMBOLS.to_string();
                    }
                }
                ui.add_space(4.0);

                ui.label(theme::small("Counter"));
                ui.horizontal(|ui| {
                    if ui.add_enabled(editor.counter > 1, theme::button(icon::MINUS, "")).clicked() {
                        editor.counter -= 1;
                    }
                    ui.add_sized(
                        [48.0, 36.0],
                        egui::DragValue::new(&mut editor.counter).range(1..=u32::MAX),
                    );
                    if ui.add(theme::button(icon::PLUS, "")).clicked() {
                        editor.counter = editor.counter.saturating_add(1);
                    }
                });
                ui.label(theme::small("Raise it to get a new password for this site."));
                ui.add_space(4.0);
                ui.label(theme::small(format!(
                    "Strength: about {:.0} bits",
                    entropy_bits(editor.length, &editor.classes(), &editor.symbol_set)
                )));
            });
        }

        if editor.index.is_some() {
            theme::note(ui, icon::WARNING, "Changing anything here changes this site's password.", theme::MUTED);
        }
        ui.add_space(4.0);

        // Work on a copy so a failed save leaves the list in memory matching the file.
        let mut updated = self.sites.clone();
        let mut selected = None;
        let mut saved = false;
        ui.horizontal(|ui| {
            let save = if editor.index.is_some() { "Save changes" } else { "Add site" };
            if ui.add(theme::primary_button("", save)).clicked() {
                let result = editor.to_site().and_then(|site| {
                    let key = (site.site.clone(), site.login.clone());
                    updated.put(site, editor.index, store::now())?;
                    updated.save(&self.sites_path)?;
                    Ok(updated.sites.iter().position(|s| (&s.site, &s.login) == (&key.0, &key.1)))
                });
                match result {
                    Ok(position) => {
                        selected = position;
                        saved = true;
                    }
                    Err(e) => {
                        // The error may be about a folded-away setting.
                        editor.expanded = true;
                        self.error = e;
                    }
                }
            }
            if ui.button("Cancel").clicked() {
                next = Some(Screen::Sites);
            }
            if let Some(i) = editor.index {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !editor.confirm_delete {
                        if ui.add(theme::danger_button(icon::TRASH, "Delete")).clicked() {
                            editor.confirm_delete = true;
                        }
                    } else if ui.add(theme::danger_button("", "Yes, delete it")).clicked() {
                        updated.remove(i, store::now());
                        match updated.save(&self.sites_path) {
                            Ok(()) => saved = true,
                            Err(e) => self.error = e,
                        }
                    }
                });
            }
        });
        if editor.confirm_delete {
            ui.label(theme::small(
                "Rune forgets this site's settings. Adding it again with the same settings gives the same password.",
            ));
        }

        if saved {
            self.sites = updated;
            self.select(selected);
            next = Some(Screen::Sites);
        }
        if back || ui.input(|i| i.key_pressed(Key::Escape)) {
            next = Some(Screen::Sites);
        }
        if next.is_some() {
            self.error.clear();
            self.focus_search = true;
        }
        next
    }

    // ---- Settings ----------------------------------------------------------------------------

    fn export_sites(&self, path: &Path) -> Result<String, String> {
        self.sites.save(path)?;
        Ok(format!("Exported {} sites to {}.", self.sites.sites.len(), path.display()))
    }

    /// A Rune site list, or a Bitwarden export.
    fn import_sites(&mut self, path: &Path) -> Result<String, String> {
        // A file that can't be read is left for `SiteList::load` below to explain.
        if let (Ok(mut text), Some(keys)) = (std::fs::read_to_string(path), &self.keys) {
            let mut sealer = seal_key(&keys.master);
            let converted = bitwarden::convert(&text, &sealer, store::nonce);
            sealer.zeroize();
            // A Bitwarden export is every password in plain text.
            text.zeroize();
            if let Some(converted) = converted? {
                return self.import_bitwarden(converted);
            }
        }

        let imported = SiteList::load(path)?;
        let mut updated = self.sites.clone();
        let (added, replaced) = updated.merge(imported);
        updated.save(&self.sites_path)?;
        self.sites = updated;
        self.select(None);
        Ok(format!("Imported: {added} new, {replaced} updated."))
    }

    /// Adds what isn't in Rune yet. An account already here is left alone: it may have been
    /// switched to Rune's password since, and the export's would undo that.
    fn import_bitwarden(&mut self, converted: bitwarden::Converted) -> Result<String, String> {
        let mut updated = self.sites.clone();
        let (mut added, mut already) = (0, 0);
        for site in converted.sites {
            match updated.put(site, None, store::now()) {
                Ok(()) => added += 1,
                Err(_) => already += 1,
            }
        }
        updated.save(&self.sites_path)?;
        self.sites = updated;
        self.select(None);

        let skipped: Vec<String> = [
            (already, "already in Rune"),
            (converted.duplicates, "listed twice"),
            (converted.no_password, "without a password"),
            (converted.not_logins, "not logins (notes, cards, identities)"),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, why)| format!("{n} {why}"))
        .collect();
        let skipped = if skipped.is_empty() { String::new() } else { format!(" Skipped: {}.", skipped.join(", ")) };
        Ok(format!(
            "Imported {added} from Bitwarden, each keeping its password until you switch it to Rune's.{skipped} \
             Now delete the export file: it holds your passwords in plain text."
        ))
    }

    fn settings_screen(&mut self, ui: &mut Ui) -> Option<Screen> {
        let mut next = None;
        let back = header(ui, "Settings");

        // A file dialog finished on its thread?
        let finished = match &mut self.screen {
            Screen::Settings(settings) => match &settings.dialog {
                Some((kind, rx)) => match rx.try_recv() {
                    Ok(path) => {
                        let kind = *kind;
                        settings.dialog = None;
                        path.map(|path| (kind, path))
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        settings.dialog = None;
                        None
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                },
                None => None,
            },
            _ => return None,
        };
        if let Some((kind, path)) = finished {
            let result = match kind {
                Dialog::Export => self.export_sites(&path),
                Dialog::Import => self.import_sites(&path),
            };
            let (message, error) = match result {
                Ok(message) => (message, String::new()),
                Err(e) => (String::new(), e),
            };
            self.error = error;
            if let Screen::Settings(settings) = &mut self.screen {
                settings.message = message;
            }
        }

        let sync_note = self.sync_note();
        let Screen::Settings(settings) = &mut self.screen else {
            return None;
        };
        let Some(keys) = &self.keys else {
            return Some(Screen::Setup);
        };

        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Your key").strong());
            ui.horizontal(|ui| {
                ui.label(theme::muted("Fingerprint"));
                ui.label(RichText::new(fingerprint(&keys.master)).monospace().color(theme::ACCENT));
            });
            ui.label(theme::small(
                "A new device needs only your master password; matching fingerprints show it was typed right.",
            ));
        });

        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Phone sync").strong());
            ui.label(theme::small(
                "Over your local network, between devices with the same master password. Nothing goes through a server.",
            ));
            ui.label(theme::small(&sync_note));
        });

        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Site list").strong());
            ui.label(theme::small(
                "Site names and settings, never passwords. Use the file to back up or to move your sites to another device. \
                 Import also takes a Bitwarden export (unencrypted .json): each login keeps its password until you switch it.",
            ));
            ui.horizontal(|ui| {
                let idle = settings.dialog.is_none();
                for (kind, glyph, label) in [
                    (Dialog::Export, icon::UPLOAD_SIMPLE, "Export…"),
                    (Dialog::Import, icon::DOWNLOAD_SIMPLE, "Import…"),
                ] {
                    if ui.add_enabled(idle, theme::button(glyph, label)).clicked() {
                        settings.dialog = Some((kind, open_dialog(kind, ui.ctx().clone())));
                    }
                }
            });
            if !settings.message.is_empty() {
                theme::note(ui, icon::CHECK_CIRCLE, &settings.message, theme::ACCENT);
            }
        });

        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Reset").strong());
            ui.label(theme::small(
                "Removes your keys from this computer. Your site list stays. To use Rune again here \
                 you need your master password.",
            ));
            ui.horizontal(|ui| {
                if !settings.confirm_reset {
                    if ui.add(theme::danger_button("", "Reset Rune")).clicked() {
                        settings.confirm_reset = true;
                    }
                } else {
                    if ui.add(theme::danger_button("", "Yes, remove my keys")).clicked() {
                        match store::delete_keys(store::ACCOUNT) {
                            Ok(()) => {
                                self.keys = None;
                                next = Some(Screen::Setup);
                            }
                            Err(e) => self.error = format!("Can't reach the system keyring: {e}"),
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        settings.confirm_reset = false;
                    }
                }
            });
        });

        ui.label(theme::small(format!(
            "Rune {}  ·  {}",
            env!("CARGO_PKG_VERSION"),
            self.sites_path.display()
        )));

        if next.is_none() && (back || ui.input(|i| i.key_pressed(Key::Escape))) {
            next = Some(self.go_to_sites());
        }
        next
    }

    // ---- Frame -------------------------------------------------------------------------------

    /// Starts answering the phone once there are keys, and takes in what a sync changed.
    fn sync_with_phone(&mut self, ctx: &egui::Context) {
        if !self.online {
            return;
        }
        match &self.keys {
            Some(keys) if self.sync.is_none() && self.sync_error.is_none() => {
                let wake = ctx.clone();
                let listener = store::listen_for_sync(
                    store::SYNC_PORT,
                    rune_core::sync_key(&keys.master),
                    &fingerprint(&keys.master),
                    self.sites_path.clone(),
                    move || wake.request_repaint(),
                );
                match listener {
                    Ok(listener) => self.sync = Some(listener),
                    Err(e) => self.sync_error = Some(e.to_string()),
                }
            }
            // The keys were removed: stop answering for them.
            None => (self.sync, self.sync_error) = (None, None),
            _ => {}
        }

        for changed in self.sync.iter().flat_map(|listener| listener.synced.try_iter()) {
            self.synced_at = Some(Instant::now());
            self.reload_sites |= changed;
        }
        // The editor holds an index into the list, so the list stays put while it's open.
        if self.reload_sites && !matches!(self.screen, Screen::Editor(_)) {
            self.reload_sites = false;
            match SiteList::load(&self.sites_path) {
                Ok(sites) => {
                    let selected = self.selected.and_then(|i| self.sites.sites.get(i)).map(|s| (s.site.clone(), s.login.clone()));
                    self.sites = sites;
                    let position = selected.and_then(|(site, login)| {
                        self.sites.sites.iter().position(|s| s.site == site && s.login == login)
                    });
                    if position != self.selected {
                        self.select(position);
                    }
                }
                Err(e) => self.error = e,
            }
        }
    }

    /// One line for Settings on where sync stands.
    fn sync_note(&self) -> String {
        match (&self.sync_error, self.synced_at) {
            (Some(e), _) => format!("Not listening for your phone: {e}."),
            (None, Some(at)) => match at.elapsed().as_secs() / 60 {
                0 => "Synced with your phone just now.".to_string(),
                1 => "Synced with your phone a minute ago.".to_string(),
                minutes => format!("Synced with your phone {minutes} minutes ago."),
            },
            (None, None) => "Open Rune on your phone, on this Wi-Fi, and the two site lists become one.".to_string(),
        }
    }

    fn draw(&mut self, ui: &mut Ui) {
        self.sync_with_phone(ui.ctx());
        let frame = egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(20));
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            // Keep a readable measure on wide windows.
            let mut rect = ui.available_rect_before_wrap();
            if rect.width() > CONTENT_WIDTH {
                rect = egui::Rect::from_center_size(rect.center(), egui::vec2(CONTENT_WIDTH, rect.height()));
            }
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| self.screen(ui));
        });
    }

    fn screen(&mut self, ui: &mut Ui) {
        // The sites screen fills the window, so its error is pinned to the bottom; every other
        // screen shows the error right under the buttons that caused it.
        let error_label = |ui: &mut Ui, error: &str| {
            ui.add_space(8.0);
            ui.add(egui::Label::new(theme::icon_text(icon::WARNING, error, theme::DANGER, 15.0)).wrap());
        };
        if matches!(self.screen, Screen::Sites) {
            if !self.error.is_empty() {
                egui::Panel::bottom("error")
                    .frame(egui::Frame::NONE)
                    .show_separator_line(false)
                    .show(ui, |ui| error_label(ui, &self.error));
            }
            if let Some(screen) = self.sites_screen(ui) {
                self.screen = screen;
            }
            return;
        }
        // Every other screen scrolls, so nothing falls off the bottom of a short window.
        let next = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let next = match &self.screen {
                Screen::Setup => self.setup_screen(ui),
                Screen::Deriving { rx } => {
                    let received = rx.try_recv().ok();
                    ui.add_space(ui.available_height() * 0.35);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.label(theme::muted("Deriving your key…"));
                    });
                    received.map(|keys| Screen::Confirm { keys })
                }
                Screen::Confirm { .. } => self.confirm_screen(ui),
                Screen::Sites => unreachable!("drawn above"),
                Screen::Editor(_) => self.editor_screen(ui),
                Screen::Settings(_) => self.settings_screen(ui),
                Screen::Fatal(message) => {
                    ui.add_space(24.0);
                    ui.heading("Rune can't start");
                    ui.label(theme::muted("The site list couldn't be read, so Rune stopped rather than overwrite it."));
                    ui.add(egui::Label::new(RichText::new(message).monospace()).wrap());
                    None
                }
            };
            if !self.error.is_empty() {
                error_label(ui, &self.error);
            }
            next
        }).inner;
        if let Some(screen) = next {
            self.screen = screen;
        }
    }
}

/// Six fresh words from the OS's randomness.
fn new_passphrase() -> String {
    passphrase(|| store::random_u16().expect("/dev/urandom is readable"))
}

/// How long a well-equipped attacker (~100,000 guesses a second: Argon2 on a rack of GPUs)
/// would take to guess `password` after a site leaks one of its passwords.
fn strength(password: &str) -> (String, egui::Color32) {
    const GUESSES_PER_SECOND: f64 = 1e5;
    let seconds = zxcvbn::zxcvbn(password, &["rune"]).guesses() as f64 / GUESSES_PER_SECOND;
    let (day, year) = (86400.0, 3.15e7);
    let about = |n: f64, unit: &str| {
        let n = n.round().max(1.0);
        format!("about {n} {unit}{}", if n == 1.0 { "" } else { "s" })
    };
    let (when, color) = match seconds {
        s if s < 3600.0 => ("under an hour".to_string(), theme::DANGER),
        s if s < day * 30.0 => (about(s / day, "day"), theme::DANGER),
        s if s < year => (about(s / (day * 30.0), "month"), theme::DANGER),
        s if s < year * 1000.0 => (about(s / year, "year"), theme::MUTED),
        _ => ("thousands of years or more".to_string(), theme::ACCENT),
    };
    (format!("If a site leaks one of your passwords, guessing this would take {when}."), color)
}

/// Back arrow and title. Returns true when back was clicked.
fn header(ui: &mut Ui, title: &str) -> bool {
    let back = ui
        .horizontal(|ui| {
            let back = theme::icon_button(ui, icon::ARROW_LEFT, "Back").clicked();
            ui.heading(title);
            back
        })
        .inner;
    ui.add_space(4.0);
    back
}

/// The site's favicon texture, loaded once; asks for it in the background the first time.
fn site_icon<'a>(
    icons: &'a mut HashMap<String, Option<(egui::TextureHandle, egui::Color32)>>,
    ctx: &egui::Context,
    site: &str,
) -> Option<&'a (egui::TextureHandle, egui::Color32)> {
    if !icons.contains_key(site) {
        let repaint = ctx.clone();
        let image = store::favicon(site, move || repaint.request_repaint())?;
        let texture = image.map(|image| {
            let tile = theme::tile_color(&image);
            (ctx.load_texture(format!("icon:{site}"), image, egui::TextureOptions::LINEAR), tile)
        });
        icons.insert(site.to_owned(), texture);
    }
    icons[site].as_ref()
}

/// The name to show for a site: the one it gives itself ("GitHub"), else the stored name, title
/// cased when it isn't an address ("My Bank"). Display only; passwords use the stored name.
fn display_name(names: &mut HashMap<String, Option<String>>, fetch: bool, ctx: &egui::Context, site: &str) -> String {
    if fetch && !names.contains_key(site) {
        let repaint = ctx.clone();
        if let Some(name) = store::site_name(site, move || repaint.request_repaint()) {
            names.insert(site.to_owned(), name);
        }
    }
    names.get(site).cloned().flatten().unwrap_or_else(|| store::title_case(site))
}

fn site_row(
    ui: &mut Ui,
    site: &Site,
    name: &str,
    icon: Option<&(egui::TextureHandle, egui::Color32)>,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 52.0), Sense::click());
    let subtitle = &site.login;
    let label = if subtitle.is_empty() { name.to_owned() } else { format!("{name}, {subtitle}") };
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &label)
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    if selected || response.hovered() || response.has_focus() {
        let fill = if selected { theme::SURFACE_HI } else { theme::SURFACE };
        painter.rect_filled(rect, theme::RADIUS, fill);
    }
    if response.has_focus() {
        painter.rect_stroke(rect, theme::RADIUS, egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
    }

    let avatar = egui::pos2(rect.left() + 30.0, rect.center().y);
    let tile = egui::Rect::from_center_size(avatar, egui::Vec2::splat(36.0));
    let wash = icon.map_or(theme::ACCENT.gamma_multiply(0.16), |(_, color)| *color);
    painter.rect_filled(tile, 11, wash);
    if let Some((icon, _)) = icon {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(icon.id(), tile.shrink(6.0), uv, egui::Color32::WHITE);
    } else {
        let initial = site.site.chars().next().unwrap_or('?').to_uppercase().to_string();
        painter.text(avatar, egui::Align2::CENTER_CENTER, initial, egui::FontId::proportional(16.0), theme::ACCENT);
    }

    let left = rect.left() + 60.0;
    if subtitle.is_empty() {
        painter.text(
            egui::pos2(left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(15.0),
            theme::TEXT,
        );
    } else {
        painter.text(
            egui::pos2(left, rect.center().y - 9.0),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(15.0),
            theme::TEXT,
        );
        painter.text(
            egui::pos2(left, rect.center().y + 10.0),
            egui::Align2::LEFT_CENTER,
            subtitle,
            egui::FontId::proportional(13.0),
            theme::MUTED,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Runs the system file dialog on its own thread so the window stays responsive.
fn open_dialog(kind: Dialog, ctx: egui::Context) -> Receiver<Option<PathBuf>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let dialog = rfd::FileDialog::new().add_filter("Rune site list or Bitwarden export", &["json"]);
        let path = match kind {
            Dialog::Export => dialog.set_file_name("rune-sites.json").save_file(),
            Dialog::Import => dialog.pick_file(),
        };
        let _ = tx.send(path);
        ctx.request_repaint();
    });
    rx
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 680.0])
            .with_min_inner_size([380.0, 520.0])
            .with_app_id("rune"),
        ..Default::default()
    };
    eframe::run_native(
        "Rune",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

#[cfg(test)]
mod tests;
