// SPDX-License-Identifier: GPL-3.0

use crate::config::Config;
use crate::fl;
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::{window::Id, Limits, Subscription};
use cosmic::iced_winit::commands::popup::{destroy_popup, get_popup};
use cosmic::prelude::*;
use cosmic::widget;
use futures_util::SinkExt;
use std::time::Duration;

const GITHUB_REVIEW_URL: &str = "https://github.com/pulls?q=is%3Apr+is%3Aopen+review-requested%3A%40me+-review%3Aapproved";
const POLL_INTERVAL_SECS: u64 = 60;

async fn fetch_pr_count() -> Result<u32, String> {
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

/// The application model stores app-specific state used to describe its interface and
/// drive its logic.
#[derive(Default)]
pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime.
    core: cosmic::Core,
    /// The popup id.
    popup: Option<Id>,
    /// Configuration data that persists between application runs.
    config: Config,
    /// Number of PRs waiting for review, or None if not yet fetched.
    pr_count: Option<u32>,
    /// Whether the last fetch resulted in an error.
    fetch_error: Option<String>,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(Config),
    PRCountFetched(Result<u32, String>),
    OpenGitHub,
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
        let app = AppModel {
            core,
            config: cosmic_config::Config::new(Self::APP_ID, Config::VERSION)
                .map(|context| match Config::get_entry(&context) {
                    Ok(config) => config,
                    Err((_errors, config)) => config,
                })
                .unwrap_or_default(),
            ..Default::default()
        };

        (app, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    /// Panel button: shows GitHub icon with PR count.
    fn view(&self) -> Element<'_, Self::Message> {
        let count_str = match (&self.fetch_error, self.pr_count) {
            (Some(_), _) => "!".to_string(),
            (_, Some(n)) => n.to_string(),
            (_, None) => "…".to_string(),
        };

        let content = widget::row()
            .push(
                widget::icon::from_name("com.laeborg.CosmicAppletGithubStatus")
                    .size(16),
            )
            .push(widget::text(count_str))
            .spacing(4)
            .align_y(cosmic::iced::Alignment::Center);

        widget::button::custom(content)
            .class(cosmic::theme::Button::AppletIcon)
            .on_press(Message::TogglePopup)
            .into()
    }

    /// Popup window: shows count, error state, and a button to open GitHub.
    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let body: Element<_> = match (&self.fetch_error, self.pr_count) {
            (Some(err), _) => widget::column()
                .push(widget::text(fl!("error-label")).size(14))
                .push(widget::text(err).size(12))
                .push(
                    widget::text(fl!("token-hint")).size(11),
                )
                .spacing(6)
                .into(),
            (_, Some(count)) => widget::column()
                .push(widget::text(fl!("pr-count-label")).size(14))
                .push(widget::text(count.to_string()).size(36))
                .spacing(4)
                .into(),
            (_, None) => widget::text(fl!("loading")).size(14).into(),
        };

        let content = widget::list_column()
            .padding(12)
            .spacing(8)
            .add(body)
            .add(
                widget::button::suggested(fl!("open-github"))
                    .on_press(Message::OpenGitHub),
            );

        self.core.applet.popup_container(content).into()
    }

    /// Background subscription: fetches PR count on startup, then every 60 seconds.
    fn subscription(&self) -> Subscription<Self::Message> {
        struct GithubPoller;

        Subscription::batch(vec![
            Subscription::run_with_id(
                std::any::TypeId::of::<GithubPoller>(),
                cosmic::iced::stream::channel(4, |mut channel| async move {
                    loop {
                        let result = fetch_pr_count().await;
                        let _ = channel.send(Message::PRCountFetched(result)).await;
                        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
                    }
                }),
            ),
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
        ])
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
                self.config = config;
            }
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
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
                        .min_width(200.0)
                        .min_height(80.0)
                        .max_height(400.0);
                    get_popup(popup_settings)
                };
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}
