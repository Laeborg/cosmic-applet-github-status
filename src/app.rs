// SPDX-License-Identifier: GPL-3.0

use crate::config::{AuthMethod, Config};
use crate::fl;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::{window::Id, Alignment, Limits, Subscription};
use cosmic::iced_winit::commands::popup::{destroy_popup, get_popup};
use cosmic::prelude::*;
use cosmic::widget;
use futures_util::SinkExt;
use std::time::Duration;

const GITHUB_REVIEW_URL: &str = "https://github.com/pulls?q=is%3Apr+is%3Aopen+review-requested%3A%40me+-review%3Aapproved";

const POLL_LABELS: &[&str] = &["30 sec", "1 min", "2 min", "5 min", "10 min", "30 min"];
const POLL_VALUES: &[u64] = &[30, 60, 120, 300, 600, 1800];

async fn fetch_pr_count(auth_method: AuthMethod, pat: String) -> Result<u32, String> {
    match auth_method {
        AuthMethod::GhCli => fetch_via_gh_cli().await,
        AuthMethod::Pat => {
            if pat.is_empty() {
                return Err("No PAT configured. Open Settings to add one.".to_string());
            }
            fetch_via_pat(&pat).await
        }
    }
}

async fn fetch_via_gh_cli() -> Result<u32, String> {
    let output = tokio::process::Command::new("gh")
        .args([
            "api",
            "search/issues",
            "--method", "GET",
            "-f", "q=is:pr is:open review-requested:@me -review:approved",
            "--jq", ".total_count",
        ])
        .output()
        .await
        .map_err(|e| format!("gh not found: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .map_err(|e| e.to_string())
}

async fn fetch_via_pat(pat: &str) -> Result<u32, String> {
    let output = tokio::process::Command::new("curl")
        .args([
            "--silent",
            "-H", &format!("Authorization: Bearer {pat}"),
            "-H", "Accept: application/vnd.github+json",
            "https://api.github.com/search/issues?q=is:pr+is:open+review-requested:@me+-review:approved",
        ])
        .output()
        .await
        .map_err(|e| format!("curl not found: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "Request failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value =
        serde_json::from_str(&stdout).map_err(|e| format!("JSON parse error: {e}"))?;

    value["total_count"].as_u64().map(|n| n as u32).ok_or_else(|| {
        value["message"]
            .as_str()
            .map(|m| format!("API error: {m}"))
            .unwrap_or_else(|| "total_count not found in response".to_string())
    })
}

async fn check_gh_status() -> Result<String, String> {
    let output = tokio::process::Command::new("gh")
        .args(["auth", "status"])
        .output()
        .await
        .map_err(|_| "gh not found or not executable".to_string())?;

    // gh auth status writes to stderr
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    for line in text.lines() {
        if line.contains("Logged in to") && line.contains("account") {
            if let Some(pos) = line.find("account ") {
                let rest = &line[pos + 8..];
                let username = rest.split_whitespace().next().unwrap_or("unknown");
                return Ok(username.to_string());
            }
        }
    }

    if !output.status.success() {
        return Err("Not logged in. Run: gh auth login".to_string());
    }

    Ok("Connected".to_string())
}

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// The popup id.
    popup: Option<Id>,
    /// Configuration data that persists between application runs.
    config: Config,
    /// Handle used for writing config changes.
    config_handler: Option<cosmic_config::Config>,
    /// Number of PRs waiting for review, or None if not yet fetched.
    pr_count: Option<u32>,
    /// Whether the last fetch resulted in an error.
    fetch_error: Option<String>,
    /// Whether the settings page is currently shown.
    show_settings: bool,
    /// Temporary state for the PAT text input field.
    pat_input: String,
    /// Result of gh auth status check (None = not yet checked).
    gh_status: Option<Result<String, String>>,
    /// Incremented to trigger a fresh gh auth status check.
    gh_check_id: u64,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            core: cosmic::Core::default(),
            popup: None,
            config: Config::default(),
            config_handler: None,
            pr_count: None,
            fetch_error: None,
            show_settings: false,
            pat_input: String::new(),
            gh_status: None,
            gh_check_id: 0,
        }
    }
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(Config),
    PRCountFetched(Result<u32, String>),
    OpenGitHub,
    // Settings
    OpenSettings,
    CloseSettings,
    SetAuthMethod(AuthMethod),
    SetPatInput(String),
    SavePat,
    SetPollInterval(usize),
    CheckGhStatus,
    GhStatusFetched(Result<String, String>),
}

/// Create a COSMIC application from the app model
impl cosmic::Application for AppModel {
    /// The async executor that will be used to run your application's commands.
    type Executor = cosmic::executor::Default;

    /// Data that your application receives to its init method.
    type Flags = ();

    /// Messages which the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = "com.laeborg.CosmicAppletGithubStatus";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    /// Initializes the application with any given flags and startup commands.
    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let config_handler = cosmic_config::Config::new(Self::APP_ID, Config::VERSION).ok();
        let mut config = config_handler
            .as_ref()
            .map(|h| match Config::get_entry(h) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();

        // Migrate: old configs may have poll_interval_secs = 0 (u64 default).
        if config.poll_interval_secs == 0 {
            config.poll_interval_secs = 60;
        }

        let pat_input = config.github_pat.clone();

        let app = AppModel {
            core,
            config,
            config_handler,
            pat_input,
            ..Default::default()
        };

        (app, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    /// Panel button: shows GitHub icon with a badge overlay for the PR count or error.
    fn view(&self) -> Element<'_, Self::Message> {
        use cosmic::iced::{
            alignment::{Horizontal, Vertical},
            Background, Border, Color, Length,
        };

        let icon_size = self.core.applet.suggested_size(true).0;

        // Wrap icon with padding: top/left=2 for breathing room, right/bottom=5
        // so the Stack has extra space for the badge to extend beyond the icon edge.
        let icon: Element<_> = widget::container(
            widget::icon::from_name("com.laeborg.CosmicAppletGithubStatus").size(icon_size),
        )
        .padding([2, 5, 5, 2])
        .into();

        // Badge: colored circle with label. Color depends on severity.
        let badge_info: Option<(String, Color)> = match (&self.fetch_error, self.pr_count) {
            (Some(_), _) => Some(("!".into(), Color::from_rgb(0.82, 0.18, 0.18))),
            (_, Some(0)) => Some(("0".into(), Color::from_rgb(0.13, 0.65, 0.30))),
            (_, Some(n)) if n <= 5 => Some((n.to_string(), Color::from_rgb(0.15, 0.45, 0.85))),
            (_, Some(n)) if n <= 10 => Some((n.to_string(), Color::from_rgb(0.80, 0.65, 0.10))),
            (_, Some(n)) => Some((n.to_string(), Color::from_rgb(0.82, 0.18, 0.18))),
            (_, None) => None,
        };

        let content: Element<_> = if let Some((label, bg_color)) = badge_info {
            let badge: Element<_> = widget::container(
                widget::text(label).size(9).class(Color::WHITE),
            )
            .width(13)
            .height(13)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .class(cosmic::theme::Container::Custom(Box::new(move |_| {
                cosmic::iced_widget::container::Style {
                    background: Some(Background::Color(bg_color)),
                    border: Border {
                        radius: 100.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })))
            .into();

            cosmic::iced::widget::Stack::new()
                .push(icon)
                .push(
                    widget::container(badge)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .align_x(Horizontal::Right)
                        .align_y(Vertical::Bottom),
                )
                .into()
        } else {
            icon
        };

        self.core
            .applet
            .button_from_element(content, true)
            .on_press(Message::TogglePopup)
            .into()
    }

    /// Popup window: dispatches to main view or settings view.
    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let content: Element<_> = if self.show_settings {
            self.settings_view()
        } else {
            self.main_view()
        };

        self.core.applet.popup_container(content).into()
    }

    /// Background subscriptions.
    fn subscription(&self) -> Subscription<Self::Message> {
        let auth_method = self.config.auth_method.clone();
        let pat = self.config.github_pat.clone();

        let interval = self.config.poll_interval_secs;

        let mut subs = vec![
            // Main PR poller — subscription ID includes all relevant config values,
            // so it restarts automatically when any of them changes.
            Subscription::run_with_id(
                (auth_method.clone(), pat.clone(), interval),
                cosmic::iced::stream::channel(4, move |mut channel| async move {
                    loop {
                        let result = fetch_pr_count(auth_method.clone(), pat.clone()).await;
                        let _ = channel.send(Message::PRCountFetched(result)).await;
                        tokio::time::sleep(Duration::from_secs(interval)).await;
                    }
                }),
            ),
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ];

        // GH auth status checker — only active when settings is open and GhCli is selected.
        // gh_check_id changes whenever a fresh check is requested, forcing a new subscription.
        if self.show_settings && matches!(self.config.auth_method, AuthMethod::GhCli) {
            let check_id = self.gh_check_id;
            subs.push(Subscription::run_with_id(
                check_id,
                cosmic::iced::stream::channel(1, |mut channel| async move {
                    let result = check_gh_status().await;
                    let _ = channel.send(Message::GhStatusFetched(result)).await;
                    // Hang after sending — subscription is dropped when settings closes
                    // or when gh_check_id changes.
                    futures_util::future::pending::<()>().await;
                }),
            ));
        }

        Subscription::batch(subs)
    }

    /// Handles messages emitted by the application and its widgets.
    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::PRCountFetched(Ok(count)) => {
                self.pr_count = Some(count);
                self.fetch_error = None;
            }
            Message::PRCountFetched(Err(err)) => {
                self.fetch_error = Some(err);
            }
            Message::OpenGitHub => {
                let _ = std::process::Command::new("xdg-open")
                    .arg(GITHUB_REVIEW_URL)
                    .spawn();
            }
            Message::UpdateConfig(config) => {
                // Don't overwrite PAT input while user is editing in settings
                if !self.show_settings {
                    self.pat_input = config.github_pat.clone();
                }
                self.config = config;
            }
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    self.show_settings = false;
                    destroy_popup(p)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(300.0)
                        .min_width(220.0)
                        .min_height(80.0)
                        .max_height(500.0);
                    get_popup(popup_settings)
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                    self.show_settings = false;
                }
            }
            Message::OpenSettings => {
                self.show_settings = true;
                self.gh_status = None;
                self.gh_check_id += 1;
            }
            Message::CloseSettings => {
                self.show_settings = false;
            }
            Message::SetAuthMethod(method) => {
                self.config.auth_method = method;
                self.gh_status = None;
                self.gh_check_id += 1;
                if let Some(handler) = &self.config_handler {
                    let _ = self.config.write_entry(handler);
                }
            }
            Message::SetPatInput(input) => {
                self.pat_input = input;
            }
            Message::SavePat => {
                self.config.github_pat = self.pat_input.clone();
                if let Some(handler) = &self.config_handler {
                    let _ = self.config.write_entry(handler);
                }
            }
            Message::SetPollInterval(idx) => {
                if let Some(&secs) = POLL_VALUES.get(idx) {
                    self.config.poll_interval_secs = secs;
                    if let Some(handler) = &self.config_handler {
                        let _ = self.config.write_entry(handler);
                    }
                }
            }
            Message::CheckGhStatus => {
                self.gh_status = None;
                self.gh_check_id += 1;
            }
            Message::GhStatusFetched(result) => {
                self.gh_status = Some(result);
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}

impl AppModel {
    /// Main popup view: shows PR count, error state, and action buttons.
    fn main_view(&self) -> Element<'_, Message> {
        let content_section: Element<_> = match (&self.fetch_error, self.pr_count) {
            (Some(err), _) => widget::settings::section()
                .add(widget::text::heading(fl!("error-label")))
                .add(widget::text(err.clone()))
                .into(),
            (_, Some(count)) => widget::settings::section()
                .add(widget::settings::item(
                    fl!("pr-count-label"),
                    widget::text(count.to_string()).size(28),
                ))
                .into(),
            (_, None) => widget::settings::section()
                .add(widget::text::body(fl!("loading")))
                .into(),
        };

        let actions: Element<_> = widget::row()
            .push(
                widget::button::suggested(fl!("open-github")).on_press(Message::OpenGitHub),
            )
            .push(widget::horizontal_space())
            .push(widget::button::standard(fl!("settings")).on_press(Message::OpenSettings))
            .into();

        widget::column()
            .push(
                widget::column()
                    .push(content_section)
                    .push(actions)
                    .spacing(8)
                    .padding(12),
            )
            .into()
    }

    /// Settings popup view: auth method selection and method-specific options.
    fn settings_view(&self) -> Element<'_, Message> {
        // Header: back button + page title
        let header: Element<_> = widget::row()
            .push(
                widget::button::text(fl!("back"))
                    .on_press(Message::CloseSettings),
            )
            .push(widget::text::heading(fl!("settings")))
            .spacing(4)
            .align_y(Alignment::Center)
            .into();

        // Auth method section with radio buttons
        let auth_section: Element<_> = widget::settings::section()
            .title(fl!("auth-method-label"))
            .add(widget::settings::item(
                fl!("auth-gh-cli"),
                widget::radio(
                    "",
                    AuthMethod::GhCli,
                    Some(self.config.auth_method),
                    Message::SetAuthMethod,
                ),
            ))
            .add(widget::settings::item(
                fl!("auth-pat"),
                widget::radio(
                    "",
                    AuthMethod::Pat,
                    Some(self.config.auth_method),
                    Message::SetAuthMethod,
                ),
            ))
            .into();

        // Method-specific section
        let method_section: Element<_> = match self.config.auth_method {
            AuthMethod::GhCli => {
                let status_text = match &self.gh_status {
                    None => fl!("gh-checking"),
                    Some(Ok(user)) => format!("Connected as @{user}"),
                    Some(Err(err)) => err.clone(),
                };
                widget::settings::section()
                    .add(widget::text(status_text))
                    .add(
                        widget::row()
                            .push(widget::horizontal_space())
                            .push(
                                widget::button::standard(fl!("check-again"))
                                    .on_press(Message::CheckGhStatus),
                            ),
                    )
                    .into()
            }
            AuthMethod::Pat => widget::settings::section()
                .title(fl!("pat-label"))
                .add(
                    widget::text_input("ghp_...", &self.pat_input)
                        .on_input(Message::SetPatInput),
                )
                .add(
                    widget::row()
                        .push(widget::horizontal_space())
                        .push(
                            widget::button::suggested(fl!("save"))
                                .on_press(Message::SavePat),
                        ),
                )
                .into(),
        };

        let selected_interval =
            POLL_VALUES.iter().position(|&v| v == self.config.poll_interval_secs);

        let general_section: Element<_> = widget::settings::section()
            .title(fl!("general-label"))
            .add(widget::settings::item(
                fl!("poll-interval-label"),
                widget::dropdown(POLL_LABELS, selected_interval, Message::SetPollInterval),
            ))
            .into();

        widget::column()
            .push(
                widget::container(header)
                    .padding([8, 16]),
            )
            .push(
                widget::column()
                    .push(auth_section)
                    .push(method_section)
                    .push(general_section)
                    .spacing(8)
                    .padding([0, 12, 12, 12]),
            )
            .into()
    }
}
