//! The Git hosting of the open repository in the interface: Settings → Hosting, "Open on …" in
//! the menus, the pull requests and the result of CI in the commit details and the log, and the
//! Create Pull Request window.

use crate::app::App;
use crate::git::{self, RefKind};
use crate::hosting::{Account, Ci, CiRun, Info, KINDS, Kind, Request, Site};
use crate::i18n::{tr, trf};
use crate::theme;
use eframe::egui::{self, Color32, RichText, Stroke, Ui, pos2, vec2};
use std::sync::mpsc::Receiver;

/// The Create Pull Request window.
pub struct NewRequest {
    pub site: Site,
    pub source: String,
    pub target: String,
    /// The branches of the remote a request can go into.
    pub targets: Vec<String>,
    pub title: String,
    pub body: String,
    /// Why it can't be created yet (the branch is not on the server) or what to know first.
    pub note: Option<String>,
    pub pushed: bool,
    pub wait: Option<Receiver<Result<Request, String>>>,
    pub error: Option<String>,
    pub made: Option<Request>,
}

/// What the hosting says about one commit.
pub struct CommitHosting {
    pub site: Site,
    pub ci: Option<CiRun>,
    pub requests: Vec<Request>,
    pub error: Option<String>,
}

pub fn ci_color(c: Ci) -> Color32 {
    match c {
        Ci::Passed => theme::GREEN(),
        Ci::Failed => theme::RED(),
        Ci::Running => theme::STAR(),
        Ci::Cancelled => theme::TEXT_DIM(),
    }
}

pub fn ci_word(c: Ci) -> &'static str {
    match c {
        Ci::Passed => tr("passed"),
        Ci::Failed => tr("failed"),
        Ci::Running => tr("running"),
        Ci::Cancelled => tr("cancelled"),
    }
}

/// The mark of a CI result: a tick, a cross, a dot (running) or a dash (cancelled) in a circle.
pub fn paint_ci(p: &egui::Painter, c: egui::Pos2, state: Ci) {
    let col = ci_color(state);
    let s = Stroke::new(1.4, col);
    p.circle_stroke(c, 5.0, Stroke::new(1.2, col));
    match state {
        Ci::Passed => {
            p.line_segment([pos2(c.x - 2.4, c.y + 0.2), pos2(c.x - 0.7, c.y + 1.9)], s);
            p.line_segment([pos2(c.x - 0.7, c.y + 1.9), pos2(c.x + 2.5, c.y - 1.7)], s);
        }
        Ci::Failed => {
            p.line_segment([pos2(c.x - 1.9, c.y - 1.9), pos2(c.x + 1.9, c.y + 1.9)], s);
            p.line_segment([pos2(c.x - 1.9, c.y + 1.9), pos2(c.x + 1.9, c.y - 1.9)], s);
        }
        Ci::Running => {
            p.circle_filled(c, 1.8, col);
        }
        Ci::Cancelled => {
            p.line_segment([pos2(c.x - 2.2, c.y), pos2(c.x + 2.2, c.y)], s);
        }
    }
}

pub fn ci_tip(run: &CiRun) -> String {
    if run.name.is_empty() { format!("CI: {}", ci_word(run.state)) } else { format!("CI: {} — {}", ci_word(run.state), run.name) }
}

fn open(ui: &Ui, url: &str) {
    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
}

/// "Open on GitHub" for a file in the menu of a file list: `rev` is a commit or a branch that
/// is on the server.
pub fn file_item(ui: &mut Ui, at: &Option<(Site, String)>, path: &str) {
    let Some((site, rev)) = at else { return };
    if ui.button(trf("Open on {}", &[&site.kind.name()])).on_hover_text(tr("The file on the hosting, in the browser")).clicked() {
        open(ui, &site.file_url(rev, path));
        ui.close();
    }
}

impl App {
    /// The hosting a remote of the open repository is on.
    pub fn site_of(&self, remote: &str) -> Option<Site> {
        if let Some(s) = &self.fake_site {
            return Some(s.clone());
        }
        let repo = self.repo.as_ref()?;
        let url = repo.state.remote_urls.iter().find(|(n, _)| n == remote).map(|(_, u)| u.as_str())?;
        Site::of(url, &self.settings.hosting)
    }

    /// The remote the current branch follows; else `origin`, else the first one.
    pub fn main_remote(&self) -> Option<String> {
        let repo = self.repo.as_ref()?;
        let remotes = &repo.state.remotes;
        let up = repo.current_branch().and_then(|b| b.upstream.as_deref()).and_then(|u| crate::hosting::split_remote(u, remotes)).map(|x| x.0.to_string());
        up.or_else(|| remotes.iter().find(|r| *r == "origin").cloned()).or_else(|| remotes.first().cloned())
    }

    pub fn site(&self) -> Option<Site> {
        self.site_of(&self.main_remote()?)
    }

    /// The remote and the name there of a branch: of a remote branch itself; of a local one the
    /// branch of its own name on the remote it follows (else on the main one, else on any). A
    /// local branch made from `origin/main` and not pushed yet follows `origin/main`, but is not it.
    pub fn branch_on_remote(&self, name: &str, kind: RefKind) -> Option<(String, String)> {
        let repo = self.repo.as_ref()?;
        let remotes = &repo.state.remotes;
        if kind == RefKind::Remote {
            return crate::hosting::split_remote(name, remotes).map(|(r, b)| (r.to_string(), b.to_string()));
        }
        let local = repo.state.branches.iter().find(|b| b.kind == RefKind::Local && b.name == name)?;
        let follows = local.upstream.as_deref().and_then(|u| crate::hosting::split_remote(u, remotes)).map(|x| x.0.to_string());
        let order = follows.into_iter().chain(self.main_remote()).chain(remotes.iter().cloned());
        let there = |r: &String| repo.state.branches.iter().any(|b| b.kind == RefKind::Remote && b.name == format!("{r}/{name}"));
        order.into_iter().find(there).map(|r| (r, name.to_string()))
    }

    /// Whether a commit is in some branch of a remote. Asked of git once per commit (the
    /// answer is dropped when the repository is read again): for the menu of a commit.
    pub fn commit_pushed(&mut self, hash: &str) -> bool {
        if let Some((h, yes)) = &self.pushed_memo {
            if h == hash {
                return *yes;
            }
        }
        let Some(repo) = self.repo.as_ref() else { return false };
        let yes = self.fake_site.is_some() || !git::run(&repo.path, &["for-each-ref", "--count=1", "--contains", hash, "refs/remotes"]).stdout.trim().is_empty();
        self.pushed_memo = Some((hash.to_string(), yes));
        yes
    }

    /// Where the files of the Changes list are on the hosting: in the branch the current one follows.
    pub fn hosting_of_changes(&self) -> Option<(Site, String)> {
        let head = self.repo.as_ref()?.state.head_branch.clone()?;
        let (remote, branch) = self.branch_on_remote(&head, RefKind::Local)?;
        Some((self.site_of(&remote)?, branch))
    }

    /// Where the files of the commit shown are on the hosting, when the commit is there: one
    /// of the branches that contain it (read with the details) is a branch of a remote.
    pub fn hosting_of_shown_commit(&self) -> Option<(Site, String)> {
        let repo = self.repo.as_ref().filter(|r| r.multi.len() <= 1)?;
        let d = repo.details.as_ref()?;
        let pushed = self.fake_site.is_some() || d.containing.iter().any(|c| crate::hosting::split_remote(c, &repo.state.remotes).is_some());
        pushed.then(|| self.site()).flatten().map(|site| (site, d.hash.clone()))
    }

    /// The results of CI by commit for the log; starts reading them when it is time.
    pub fn hosting_tick(&mut self, ctx: &egui::Context) -> Option<Site> {
        let site = self.site()?;
        self.hosting.info(&site, ctx);
        Some(site)
    }

    /// What the hosting says about a commit: CI and the open requests it belongs to.
    pub fn hosting_for_commit(&mut self, ctx: &egui::Context, hash: &str, containing: &[String]) -> Option<CommitHosting> {
        let remote = self.main_remote()?;
        let site = self.site_of(&remote)?;
        let info = self.hosting.info(&site, ctx)?;
        // A request shows at its top commit and at the commits that are in its branch but not
        // yet in the branch it goes into.
        let has = |b: &str| containing.iter().any(|c| *c == format!("{remote}/{b}"));
        let requests = info.requests.iter().filter(|r| r.sha == hash || has(&r.source) && !has(&r.target)).take(3).cloned().collect();
        Some(CommitHosting { ci: info.ci.get(hash).cloned(), requests, error: info.error.clone(), site })
    }

    /// "Open on …" for a commit, in its menu.
    pub fn hosting_commit_item(&mut self, ui: &mut Ui, hash: &str) {
        let Some(site) = self.site() else { return };
        ui.separator();
        let pushed = self.commit_pushed(hash);
        if ui.add_enabled(pushed, egui::Button::new(trf("Open on {}", &[&site.kind.name()]))).on_disabled_hover_text(tr("The commit is not on the server yet")).clicked() {
            open(ui, &site.commit_url(hash));
            ui.close();
        }
    }

    /// The hosting's part of a branch menu: its page, its open requests, a new request.
    pub fn hosting_branch_items(&mut self, ui: &mut Ui, name: &str, kind: RefKind) {
        if kind == RefKind::Tag {
            return;
        }
        let Some((remote, branch)) = self.branch_on_remote(name, kind) else {
            // A local branch nobody pushed: say what is needed, where there is a hosting at all.
            if let Some(site) = self.site() {
                ui.separator();
                let _ = ui.add_enabled(false, egui::Button::new(format!("{}…", trf("Create {}", &[&tr(site.kind.request())])))).on_disabled_hover_text(tr("Push the branch first"));
            }
            return;
        };
        let Some(site) = self.site_of(&remote) else { return };
        let info: Option<&Info> = self.hosting.info(&site, ui.ctx());
        let requests: Vec<Request> = info.map(|i| i.requests_of(&branch).into_iter().cloned().collect()).unwrap_or_default();
        let is_default = info.is_some_and(|i| i.default_branch == branch);
        ui.separator();
        if ui.button(trf("Open on {}", &[&site.kind.name()])).clicked() {
            open(ui, &site.branch_url(&branch));
            ui.close();
        }
        for r in &requests {
            let mut title: String = r.title.chars().take(48).collect();
            if title.len() < r.title.len() {
                title.push('…');
            }
            if ui.button(format!("{} {}{}: {title}", tr(site.kind.request()), site.kind.sign(), r.number)).on_hover_text(format!("{} → {}\n{}", r.source, r.target, r.url)).clicked() {
                open(ui, &r.url);
                ui.close();
            }
        }
        if requests.is_empty() && !is_default && ui.button(format!("{}…", trf("Create {}", &[&tr(site.kind.request())]))).clicked() {
            self.open_new_request(&remote, &branch);
            ui.close();
        }
    }

    /// Git → Create Pull Request…: for the current branch.
    pub fn hosting_menu_item(&mut self, ui: &mut Ui) {
        let Some(site) = self.site() else { return };
        let head = self.repo.as_ref().and_then(|r| r.state.head_branch.clone());
        let at = head.as_deref().and_then(|h| self.branch_on_remote(h, RefKind::Local));
        let label = format!("{}…", trf("Create {}", &[&tr(site.kind.request())]));
        let tip = if head.is_none() { tr("HEAD is detached: check out a branch first") } else { tr("Push the branch first") };
        if ui.add_enabled(at.is_some(), egui::Button::new(label)).on_hover_text(tr("Ask to merge the current branch into another one on the hosting")).on_disabled_hover_text(tip).clicked() {
            if let Some((remote, branch)) = at {
                self.open_new_request(&remote, &branch);
            }
            ui.close();
        }
    }

    /// Whether the Push window may offer a request for this branch, and what it is called there.
    pub fn request_after_push(&self, remote: &str, branch: &str) -> Option<&'static str> {
        let site = self.site_of(remote)?;
        let info = self.hosting.known(&site);
        let main = info.map(|i| i.default_branch.clone()).filter(|b| !b.is_empty()).or_else(|| self.default_branch(remote));
        if main.as_deref() == Some(branch) || info.is_some_and(|i| !i.requests_of(branch).is_empty()) {
            return None;
        }
        Some(site.kind.request())
    }

    /// The branch `<remote>/HEAD` points at; else `main` or `master`, whichever the remote has.
    fn default_branch(&self, remote: &str) -> Option<String> {
        let repo = self.repo.as_ref()?;
        let head = git::run(&repo.path, &["symbolic-ref", "--quiet", "--short", &format!("refs/remotes/{remote}/HEAD")]).stdout;
        if let Some(b) = head.trim().strip_prefix(&format!("{remote}/")) {
            return Some(b.to_string());
        }
        ["main", "master", "develop"].into_iter().find(|b| repo.state.branches.iter().any(|x| x.kind == RefKind::Remote && x.name == format!("{remote}/{b}"))).map(str::to_string)
    }

    pub fn open_new_request(&mut self, remote: &str, branch: &str) {
        let Some(site) = self.site_of(remote) else { return };
        let Some(repo) = self.repo.as_ref() else { return };
        let prefix = format!("{remote}/");
        let targets: Vec<String> = repo
            .state
            .branches
            .iter()
            .filter(|b| b.kind == RefKind::Remote)
            .filter_map(|b| b.name.strip_prefix(&prefix))
            .filter(|b| *b != "HEAD" && *b != branch)
            .map(str::to_string)
            .collect();
        let known = self.hosting.known(&site).map(|i| i.default_branch.clone()).filter(|b| !b.is_empty());
        let target = known.or_else(|| self.default_branch(remote)).filter(|t| t != branch).or_else(|| targets.first().cloned()).unwrap_or_default();
        let remote_ref = format!("{remote}/{branch}");
        let pushed = self.fake_site.is_some() || repo.state.branches.iter().any(|b| b.kind == RefKind::Remote && b.name == remote_ref);
        // What the request would hold: the commits of the branch that the target does not have.
        let subjects: Vec<String> = if pushed && !target.is_empty() {
            git::run(&repo.path, &["log", "--reverse", "--format=%s", "-n", "30", &format!("{remote}/{target}..{remote_ref}")]).stdout.lines().map(str::to_string).collect()
        } else {
            vec![]
        };
        let (title, body) = match subjects.as_slice() {
            [one] => (one.clone(), String::new()),
            [] => (branch.to_string(), String::new()),
            many => (branch.to_string(), many.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n")),
        };
        let ahead = repo.state.branches.iter().find(|b| b.kind == RefKind::Local && b.upstream.as_deref() == Some(remote_ref.as_str())).map_or(0, |b| b.ahead);
        let note = if !pushed {
            Some(trf("The branch is not on {} yet: push it first.", &[&remote]))
        } else if ahead > 0 {
            Some(trf("{} of the branch are not pushed yet and will not be in the request.", &[&format!("{ahead} {}", crate::i18n::plural(ahead as usize, "commit", "commits"))]))
        } else {
            None
        };
        self.new_request = Some(NewRequest { site, source: branch.to_string(), target, targets, title, body, note, pushed, wait: None, error: None, made: None });
    }

    pub fn new_request_window(&mut self, ctx: &egui::Context) {
        let Some(mut f) = self.new_request.take() else { return };
        let mut keep = true;
        let mut create = false;
        let word = f.site.kind.request();
        if let Some(rx) = &f.wait {
            if let Ok(r) = rx.try_recv() {
                f.wait = None;
                match r {
                    Ok(made) => {
                        self.hosting.add_request(&f.site, made.clone());
                        f.made = Some(made);
                    }
                    Err(e) => f.error = Some(e),
                }
            }
        }
        let connected = f.site.connected();
        let ready = f.pushed && !f.title.trim().is_empty() && !f.target.is_empty() && f.target != f.source && f.wait.is_none();
        let closed = crate::form::show(ctx, "newrequest", &trf("Create {}", &[&tr(word)]), 560.0, |ui, part| {
            if part == crate::form::Part::Footer {
                if let Some(made) = &f.made {
                    if crate::form::button(ui, tr("Close")) {
                        keep = false;
                    }
                    if crate::form::primary(ui, tr("Open in Browser"), true, true) {
                        open(ui, &made.url);
                        keep = false;
                    }
                    return;
                }
                if ui.add_enabled_ui(f.wait.is_none(), |ui| crate::form::button(ui, tr("Cancel"))).inner {
                    keep = false;
                }
                if connected {
                    if crate::form::primary(ui, tr("Create"), ready, false) {
                        create = true;
                    }
                    if crate::form::button(ui, tr("Open in Browser")) {
                        open(ui, &f.site.new_request_url(&f.source, &f.target));
                    }
                    if f.wait.is_some() {
                        ui.spinner();
                    }
                } else if crate::form::primary(ui, tr("Open in Browser"), f.pushed && !f.target.is_empty(), true) {
                    open(ui, &f.site.new_request_url(&f.source, &f.target));
                    keep = false;
                }
                return;
            }
            if let Some(made) = &f.made {
                ui.label(RichText::new(format!("✓ {} {}{}", tr(word), f.site.kind.sign(), made.number)).color(theme::GREEN()).size(14.0));
                ui.add_space(4.0);
                ui.label(RichText::new(&made.title).color(theme::TEXT()));
                ui.add_space(4.0);
                ui.hyperlink(&made.url);
                return;
            }
            egui::Grid::new("newrequest_grid").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
                ui.label(tr("Repository:"));
                ui.label(RichText::new(format!("{} — {}", f.site.kind.name(), f.site.id())).color(theme::TEXT_MID()));
                ui.end_row();
                ui.label(tr("From:"));
                ui.label(RichText::new(&f.source).monospace());
                ui.end_row();
                ui.label(tr("Into:"));
                egui::ComboBox::from_id_salt("newrequest_target").width(260.0).selected_text(if f.target.is_empty() { "—" } else { f.target.as_str() }).show_ui(ui, |ui| {
                    for t in &f.targets {
                        ui.selectable_value(&mut f.target, t.clone(), t);
                    }
                });
                ui.end_row();
            });
            if let Some(n) = &f.note {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(n).color(theme::STAR()).size(12.0)).wrap());
            }
            if !connected {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(trf("There is no token for {} in Settings → Hosting, so the request is finished on the page of the hosting in the browser.", &[&f.site.host])).color(theme::TEXT_MID()).size(12.0)).wrap());
                return;
            }
            ui.add_space(10.0);
            ui.label(tr("Title:"));
            theme::edit_menu(ui.add(egui::TextEdit::singleline(&mut f.title).desired_width(f32::INFINITY)));
            ui.add_space(6.0);
            ui.label(tr("Description:"));
            egui::ScrollArea::vertical().max_height(150.0 + crate::form::grow(ui)).show(ui, |ui| {
                theme::edit_menu(ui.add(egui::TextEdit::multiline(&mut f.body).desired_rows(7).desired_width(f32::INFINITY)));
            });
            if let Some(e) = &f.error {
                ui.add_space(6.0);
                ui.add(egui::Label::new(RichText::new(format!("{}: {e}", tr("Could not create"))).color(theme::RED())).wrap());
            }
        });
        if create {
            let (tx, rx) = std::sync::mpsc::channel();
            let (site, title, body, source, target, ctx) = (f.site.clone(), f.title.trim().to_string(), f.body.trim().to_string(), f.source.clone(), f.target.clone(), ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(crate::hosting::create(&site, &title, &body, &source, &target));
                ctx.request_repaint();
            });
            f.wait = Some(rx);
            f.error = None;
        }
        // An answer on its way is waited for: the request may already be made on the server.
        if keep && !closed || f.wait.is_some() {
            self.new_request = Some(f);
        } else if f.made.is_some() {
            // Read the list again: the request is there now, and CI may have started.
            self.hosting.forget();
        }
    }

    /// Settings → Hosting.
    pub fn page_hosting(&mut self, ui: &mut Ui) {
        let Some(d) = self.settings_draft.as_mut() else { return };
        ui.label(
            RichText::new(tr("GitHub, GitLab and Bitbucket are recognised by the address of the remote: github.com, gitlab.com, bitbucket.org and hosts named gitlab.… or github.… need nothing here for \"Open on …\". Add a host to say what runs on a server of your own, and enter a token to see the open pull requests and the results of CI and to create pull requests from the program."))
                .color(theme::TEXT_MID()),
        );
        ui.add_space(10.0);
        let mut remove: Option<usize> = None;
        let mut check: Option<usize> = None;
        let busy = d.host_wait.is_some();
        for (i, a) in d.s.hosting.iter_mut().enumerate() {
            let mut changed = false;
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt(("hosting_kind", i)).width(96.0).selected_text(a.kind.name()).show_ui(ui, |ui| {
                    for k in KINDS {
                        changed |= ui.selectable_value(&mut a.kind, k, k.name()).changed();
                    }
                });
                let r = ui.add(egui::TextEdit::singleline(&mut a.host).hint_text(a.kind.public_host()).desired_width(190.0));
                changed |= r.changed();
                theme::edit_menu(r);
                let r = ui.add(egui::TextEdit::singleline(&mut a.token).password(!d.show_token).hint_text(tr("access token")).desired_width(210.0));
                changed |= r.changed();
                theme::edit_menu(r);
                if ui.add_enabled(!busy, egui::Button::new(tr("Test"))).on_hover_text(tr("Sign in with the token and say whose it is")).clicked() {
                    check = Some(i);
                }
                if ui.button("✕").on_hover_text(tr("Remove this host")).clicked() {
                    remove = Some(i);
                }
            });
            if a.kind == Kind::Bitbucket {
                ui.horizontal(|ui| {
                    ui.add_space(104.0);
                    let r = ui.add(egui::TextEdit::singleline(&mut a.email).hint_text(tr("Atlassian account e-mail (for an API token)")).desired_width(408.0));
                    changed |= r.changed();
                    theme::edit_menu(r);
                });
            }
            if changed && d.host_check.as_ref().is_some_and(|c| c.0 == i) {
                d.host_check = None;
            }
            if let Some((_, res)) = d.host_check.as_ref().filter(|c| c.0 == i) {
                ui.horizontal(|ui| {
                    ui.add_space(104.0);
                    match res {
                        Ok(v) => ui.label(RichText::new(format!("✓ {v}")).color(theme::GREEN())),
                        Err(e) => ui.add(egui::Label::new(RichText::new(format!("{}: {e}", tr("Could not connect"))).color(theme::RED())).wrap()),
                    };
                });
            }
            if d.host_wait.as_ref().is_some_and(|w| w.0 == i) {
                ui.horizontal(|ui| {
                    ui.add_space(104.0);
                    ui.spinner();
                    ui.label(RichText::new(tr("Connecting…")).color(theme::TEXT_MID()));
                });
            }
            ui.add_space(4.0);
        }
        if d.s.hosting.is_empty() {
            ui.label(RichText::new(tr("No hosts yet.")).color(theme::TEXT_DIM()));
            ui.add_space(4.0);
        }
        ui.horizontal(|ui| {
            if ui.button(tr("Add Host")).clicked() {
                // The first one is most likely the public host of the kind not listed yet.
                let kind = KINDS.into_iter().find(|k| !d.s.hosting.iter().any(|a| a.kind == *k)).unwrap_or_default();
                let host = if d.s.hosting.iter().any(|a| a.host == kind.public_host()) { String::new() } else { kind.public_host().to_string() };
                d.s.hosting.push(Account { kind, host, ..Default::default() });
            }
            if !d.s.hosting.is_empty() && ui.button(if d.show_token { tr("Hide Tokens") } else { tr("Show Tokens") }).clicked() {
                d.show_token = !d.show_token;
            }
        });
        if let Some(i) = remove {
            d.s.hosting.remove(i);
            d.host_check = None;
            d.host_wait = None;
        }
        if let Some(i) = check {
            let account = d.s.hosting[i].clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(crate::hosting::test(&account));
                ctx.request_repaint();
            });
            d.host_wait = Some((i, rx));
            d.host_check = None;
        }
        if let Some((i, rx)) = &d.host_wait {
            if let Ok(r) = rx.try_recv() {
                d.host_check = Some((*i, r));
                d.host_wait = None;
            }
        }
        ui.add_space(10.0);
        ui.label(
            RichText::new(tr("Tokens. GitHub: a fine-grained token with Pull requests (read and write) and Actions (read), or a classic one with the repo scope. GitLab: a personal, project or group access token with the api scope. Bitbucket Cloud: an API token with the pull request, repository and pipeline scopes together with the e-mail of the account, or a repository or workspace access token without an e-mail. A token is sent only to the host in its row and is kept in the settings file on this computer."))
                .color(theme::TEXT_DIM())
                .size(12.0),
        );
    }
}

/// The lines of the hosting in the commit details: the result of CI and the requests.
pub fn commit_rows(ui: &mut Ui, h: &CommitHosting) {
    if h.ci.is_none() && h.requests.is_empty() && h.error.is_none() {
        return;
    }
    ui.add_space(8.0);
    if let Some(run) = &h.ci {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
            paint_ci(ui.painter(), r.center(), run.state);
            ui.add(egui::Hyperlink::from_label_and_url(RichText::new(format!("CI: {}", ci_word(run.state))).size(12.0).color(theme::LINK()), &run.url)).on_hover_text(&run.url);
            if !run.name.is_empty() {
                ui.add(egui::Label::new(RichText::new(&run.name).size(12.0).color(theme::TEXT_MID())).truncate());
            }
        });
    }
    for r in &h.requests {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let label = format!("{} {}{}", tr(h.site.kind.request()), h.site.kind.sign(), r.number);
            ui.add(egui::Hyperlink::from_label_and_url(RichText::new(label).size(12.0).color(theme::LINK()), &r.url)).on_hover_text(format!("{} → {}\n{}\n{}", r.source, r.target, r.author, r.url));
            if r.draft {
                ui.label(RichText::new(tr("draft")).size(11.5).color(theme::TEXT_DIM()));
            }
            ui.add(egui::Label::new(RichText::new(&r.title).size(12.0).color(theme::TEXT_MID())).truncate());
        });
    }
    if let Some(e) = &h.error {
        ui.add(egui::Label::new(RichText::new(format!("{}: {e}", h.site.kind.name())).size(12.0).color(theme::RED())).wrap())
            .on_hover_text(tr("Check the host and the token in File → Settings → Hosting"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testrepo::TestRepo;

    #[test]
    fn finds_the_hosting_of_a_real_repository() {
        // The "server" is a bare repository next door; git is told to read the hosting's
        // address from it, as a company mirror would be set up.
        let server = TestRepo::new("hosting-server");
        server.commit_file("a.txt", "one\n", "First");
        let r = TestRepo::new("hosting");
        let local = server.dir.display().to_string().replace('\\', "/");
        r.git(&["remote", "add", "origin", "git@github.com:octo/site.git"]);
        r.git(&["config", &format!("url.{local}.insteadOf"), "git@github.com:octo/site.git"]);
        r.git(&["fetch", "-q", "origin"]);
        let main = server.branch();
        r.git(&["checkout", "-q", "-b", "feature/search", &format!("origin/{main}")]);
        r.commit_file("b.txt", "two\n", "Add the search");
        r.commit_file("c.txt", "three\n", "Show the results");
        let mut app = r.app();
        // Whatever hosts this computer's own settings name are not part of the test.
        app.settings.hosting.clear();
        // The address git really uses is the folder: not a hosting, so nothing is offered.
        assert_eq!(app.site(), None);
        r.git(&["config", "--unset", &format!("url.{local}.insteadOf")]);
        r.git(&["config", &format!("url.{local}.pushInsteadOf"), "git@github.com:octo/site.git"]);
        app.refresh();
        let site = app.site().unwrap();
        assert_eq!((site.kind, site.id(), site.connected()), (Kind::GitHub, "github.com/octo/site".to_string(), false));
        // Not pushed: it follows the main branch of the server, but is not that branch.
        assert_eq!(app.branch_on_remote("feature/search", RefKind::Local), None);
        assert_eq!(app.hosting_of_changes(), None);
        r.git(&["push", "-q", "-u", "origin", "feature/search"]);
        app.refresh();
        assert_eq!(app.branch_on_remote("feature/search", RefKind::Local), Some(("origin".into(), "feature/search".into())));
        assert_eq!(app.branch_on_remote("origin/feature/search", RefKind::Remote), Some(("origin".into(), "feature/search".into())));
        assert_eq!(app.request_after_push("origin", "feature/search"), Some("Pull Request"));
        assert_eq!(app.request_after_push("origin", &main), None);
        assert!(app.commit_pushed(&r.head()));
        assert_eq!(app.hosting_of_changes().map(|x| x.1), Some("feature/search".to_string()));
        app.open_new_request("origin", "feature/search");
        let f = app.new_request.take().unwrap();
        assert_eq!((f.source.as_str(), f.target.as_str(), f.title.as_str(), f.body.as_str(), f.pushed, f.note.is_none()), ("feature/search", main.as_str(), "feature/search", "- Add the search\n- Show the results", true, true));
        assert_eq!(f.targets, vec![main.clone()]);
        // With a token for the host the open request of the branch is known, and no other is offered.
        app.settings.hosting = vec![Account { kind: Kind::GitHub, host: "github.com".into(), token: "t".into(), email: String::new() }];
        let site = app.site().unwrap();
        assert!(site.connected());
        let open = Request { number: "5".into(), source: "feature/search".into(), target: main.clone(), ..Default::default() };
        app.hosting.put(&site, Info { default_branch: main.clone(), requests: vec![open], ..Default::default() });
        assert_eq!(app.request_after_push("origin", "feature/search"), None);
        let ctx = egui::Context::default();
        let h = app.hosting_for_commit(&ctx, &r.head(), &["feature/search".into(), "origin/feature/search".into()]).unwrap();
        assert_eq!(h.requests.len(), 1);
        let h = app.hosting_for_commit(&ctx, "0000", &["origin/feature/search".into(), format!("origin/{main}")]).unwrap();
        assert!(h.requests.is_empty());
    }
}
