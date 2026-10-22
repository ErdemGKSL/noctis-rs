//! The run builders: one per interaction kind, each ending in `run()`.

use std::sync::Arc;

use twilight_model::id::Id;
use twilight_model::id::marker::{
    ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker,
};

use super::command::{self, ChatInput, Target};
use super::component::{self, Picks};
use super::{Dispatch, TestApp, Who};
use crate::TestError;
use crate::response::{ModalForm, Response, TestComponent};
use crate::seed::TestOption;

/// A slash-command run, from [`TestApp::command`].
#[derive(Debug)]
#[must_use = "a run does nothing until `.run().await`"]
pub struct CommandRun<'a> {
    app: &'a TestApp,
    path: String,
    who: Who,
    options: Vec<(String, TestOption)>,
}

impl<'a> CommandRun<'a> {
    pub(super) fn new(app: &'a TestApp, path: &str) -> Self {
        Self {
            app,
            path: path.to_owned(),
            who: Who::default(),
            options: Vec::new(),
        }
    }

    /// Runs as `user` (default [`TestApp::DEFAULT_USER`]).
    pub fn by(mut self, user: Id<UserMarker>) -> Self {
        self.who.by = Some(user);
        self
    }

    /// Sets the User Locale (default `en-US`).
    pub fn locale(mut self, locale: &str) -> Self {
        self.who.locale = Some(locale.to_owned());
        self
    }

    /// Runs in `guild` (default: a DM).
    pub fn in_guild(mut self, guild: Id<GuildMarker>) -> Self {
        self.who.guild = Some(guild);
        self
    }

    /// Runs in `channel` (default: the guild's first text channel).
    pub fn in_channel(mut self, channel: Id<ChannelMarker>) -> Self {
        self.who.channel = Some(channel);
        self
    }

    /// Sets option `name` (a later value for the same name wins).
    pub fn option(mut self, name: &str, value: impl Into<TestOption>) -> Self {
        self.options.push((name.to_owned(), value.into()));
        self
    }

    /// Sends the command and waits for its pipeline to end.
    ///
    /// # Errors
    ///
    /// The check errors (an unseeded id, an unknown command or option, a
    /// value Discord would reject) before anything is sent; then
    /// [`TestError::Stopped`] or [`TestError::Timeout`].
    pub async fn run(self) -> Result<Response, TestError> {
        let ctx = self.app.resolve(&self.who)?;
        let data = command::chat_input(
            self.app.definitions(),
            &ChatInput {
                path: &self.path,
                options: &self.options,
                focus: None,
            },
            self.app.seeds(),
            &ctx,
        )?;
        self.app
            .execute(Dispatch {
                kind: 2,
                data,
                ctx,
                source: None,
            })
            .await
    }
}

/// A context-menu run, from [`TestApp::user_command`] or
/// [`TestApp::message_command`].
#[derive(Debug)]
#[must_use = "a run does nothing until `.run().await`"]
pub struct MenuRun<'a> {
    app: &'a TestApp,
    name: String,
    kind: u64,
    who: Who,
    target: Target,
}

impl<'a> MenuRun<'a> {
    pub(super) fn new(app: &'a TestApp, name: &str, kind: u64) -> Self {
        Self {
            app,
            name: name.to_owned(),
            kind,
            who: Who::default(),
            target: Target::default(),
        }
    }

    /// Runs as `user` (default [`TestApp::DEFAULT_USER`]).
    pub fn by(mut self, user: Id<UserMarker>) -> Self {
        self.who.by = Some(user);
        self
    }

    /// Sets the User Locale (default `en-US`).
    pub fn locale(mut self, locale: &str) -> Self {
        self.who.locale = Some(locale.to_owned());
        self
    }

    /// Runs in `guild` (default: a DM).
    pub fn in_guild(mut self, guild: Id<GuildMarker>) -> Self {
        self.who.guild = Some(guild);
        self
    }

    /// Runs in `channel` (default: the guild's first text channel).
    pub fn in_channel(mut self, channel: Id<ChannelMarker>) -> Self {
        self.who.channel = Some(channel);
        self
    }

    /// The target of a user command (a seeded user).
    pub fn target_user(mut self, user: Id<UserMarker>) -> Self {
        self.target.user = Some(user);
        self
    }

    /// The target of a message command.
    pub fn target_message(mut self, message: Id<MessageMarker>) -> Self {
        self.target.message = Some(message);
        self
    }

    /// Sends the context-menu command and waits for its pipeline to end.
    ///
    /// # Errors
    ///
    /// [`TestError::UnknownCommand`], [`TestError::MissingOption`] without
    /// a target, [`TestError::InvalidOption`] for the other kind's target,
    /// [`TestError::Unknown`] for unseeded ids; then
    /// [`TestError::Stopped`] or [`TestError::Timeout`].
    pub async fn run(self) -> Result<Response, TestError> {
        let ctx = self.app.resolve(&self.who)?;
        let data = command::context_menu(
            self.app.definitions(),
            &self.name,
            self.kind,
            self.target,
            self.app.seeds(),
            &ctx,
        )?;
        self.app
            .execute(Dispatch {
                kind: 2,
                data,
                ctx,
                source: None,
            })
            .await
    }
}

/// A button click, from [`TestApp::click`].
#[derive(Debug)]
#[must_use = "a run does nothing until `.run().await`"]
pub struct ClickRun<'a> {
    app: &'a TestApp,
    component: TestComponent,
    who: Who,
}

impl<'a> ClickRun<'a> {
    pub(super) fn new(app: &'a TestApp, component: TestComponent) -> Self {
        Self {
            app,
            component,
            who: Who::default(),
        }
    }

    /// Clicks as `user` (default: the user of the run that sent the
    /// message).
    pub fn by(mut self, user: Id<UserMarker>) -> Self {
        self.who.by = Some(user);
        self
    }

    /// Sets the User Locale (default: the producing run's).
    pub fn locale(mut self, locale: &str) -> Self {
        self.who.locale = Some(locale.to_owned());
        self
    }

    /// Sends the click and waits for its pipeline to end.
    ///
    /// # Errors
    ///
    /// [`TestError::WrongComponent`], [`TestError::NotInteractive`],
    /// [`TestError::Disabled`], [`TestError::NotVisible`] before anything
    /// is sent; then [`TestError::Stopped`] or [`TestError::Timeout`].
    pub async fn run(self) -> Result<Response, TestError> {
        let source = Arc::clone(&self.component.source);
        let ctx = self.app.resolve_on(&source.ctx, &self.who)?;
        let data = component::click(&self.component, ctx.user)?;
        self.app
            .execute(Dispatch {
                kind: 3,
                data,
                ctx,
                source: Some(source),
            })
            .await
    }
}

/// A select-menu pick, from [`TestApp::select`].
#[derive(Debug)]
#[must_use = "a run does nothing until `.run().await`"]
pub struct SelectRun<'a> {
    app: &'a TestApp,
    component: TestComponent,
    who: Who,
    picks: Picks,
}

impl<'a> SelectRun<'a> {
    pub(super) fn new(app: &'a TestApp, component: TestComponent) -> Self {
        Self {
            app,
            component,
            who: Who::default(),
            picks: Picks::default(),
        }
    }

    /// Selects as `user` (default: the user of the run that sent the
    /// message).
    pub fn by(mut self, user: Id<UserMarker>) -> Self {
        self.who.by = Some(user);
        self
    }

    /// Sets the User Locale (default: the producing run's).
    pub fn locale(mut self, locale: &str) -> Self {
        self.who.locale = Some(locale.to_owned());
        self
    }

    /// The string-select values to pick.
    pub fn values<I: IntoIterator<Item = impl Into<String>>>(mut self, values: I) -> Self {
        self.picks.values = values.into_iter().map(Into::into).collect();
        self
    }

    /// The users to pick (user and mentionable selects).
    pub fn users(mut self, users: impl IntoIterator<Item = Id<UserMarker>>) -> Self {
        self.picks.users = users.into_iter().collect();
        self
    }

    /// The roles to pick (role and mentionable selects).
    pub fn roles(mut self, roles: impl IntoIterator<Item = Id<RoleMarker>>) -> Self {
        self.picks.roles = roles.into_iter().collect();
        self
    }

    /// The channels to pick (channel selects).
    pub fn channels(mut self, channels: impl IntoIterator<Item = Id<ChannelMarker>>) -> Self {
        self.picks.channels = channels.into_iter().collect();
        self
    }

    /// Sends the pick and waits for its pipeline to end.
    ///
    /// # Errors
    ///
    /// [`TestError::WrongComponent`], [`TestError::Disabled`],
    /// [`TestError::NotVisible`], [`TestError::SelectValues`],
    /// [`TestError::NotAnOption`], [`TestError::Unknown`] before anything is
    /// sent; then [`TestError::Stopped`] or [`TestError::Timeout`].
    pub async fn run(self) -> Result<Response, TestError> {
        let source = Arc::clone(&self.component.source);
        let ctx = self.app.resolve_on(&source.ctx, &self.who)?;
        let data = component::select(&self.component, &self.picks, self.app.seeds(), &ctx)?;
        self.app
            .execute(Dispatch {
                kind: 3,
                data,
                ctx,
                source: Some(source),
            })
            .await
    }
}

/// A modal submit, from [`TestApp::submit`].
#[derive(Debug)]
#[must_use = "a run does nothing until `.run().await`"]
pub struct SubmitRun<'a> {
    app: &'a TestApp,
    form: ModalForm,
    who: Who,
}

impl<'a> SubmitRun<'a> {
    pub(super) fn new(app: &'a TestApp, form: ModalForm) -> Self {
        Self {
            app,
            form,
            who: Who::default(),
        }
    }

    /// Submits as `user` (default: the user of the run that opened the
    /// modal).
    pub fn by(mut self, user: Id<UserMarker>) -> Self {
        self.who.by = Some(user);
        self
    }

    /// Sets the User Locale (default: the opening run's).
    pub fn locale(mut self, locale: &str) -> Self {
        self.who.locale = Some(locale.to_owned());
        self
    }

    /// Sends the submit and waits for its pipeline to end. A modal opened
    /// from a component keeps that component's message as the submit's
    /// `message`.
    ///
    /// # Errors
    ///
    /// [`TestError::UnknownInput`], [`TestError::WrongInput`],
    /// [`TestError::InvalidInput`], [`TestError::Unsupported`] (file
    /// uploads) before anything is sent; then [`TestError::Stopped`] or
    /// [`TestError::Timeout`].
    pub async fn run(self) -> Result<Response, TestError> {
        let ctx = self.app.resolve_on(&self.form.ctx, &self.who)?;
        let data = component::submit(&self.form, self.app.seeds(), &ctx)?;
        self.app
            .execute(Dispatch {
                kind: 5,
                data,
                ctx,
                source: self.form.source.clone(),
            })
            .await
    }
}

/// An autocomplete run, from [`TestApp::autocomplete`].
#[derive(Debug)]
#[must_use = "a run does nothing until `.run().await`"]
pub struct AutocompleteRun<'a> {
    app: &'a TestApp,
    path: String,
    focused: String,
    typed: String,
    who: Who,
    options: Vec<(String, TestOption)>,
}

impl<'a> AutocompleteRun<'a> {
    pub(super) fn new(app: &'a TestApp, path: &str, option: &str, typed: &str) -> Self {
        Self {
            app,
            path: path.to_owned(),
            focused: option.to_owned(),
            typed: typed.to_owned(),
            who: Who::default(),
            options: Vec::new(),
        }
    }

    /// Runs as `user` (default [`TestApp::DEFAULT_USER`]).
    pub fn by(mut self, user: Id<UserMarker>) -> Self {
        self.who.by = Some(user);
        self
    }

    /// Sets the User Locale (default `en-US`).
    pub fn locale(mut self, locale: &str) -> Self {
        self.who.locale = Some(locale.to_owned());
        self
    }

    /// Runs in `guild` (default: a DM).
    pub fn in_guild(mut self, guild: Id<GuildMarker>) -> Self {
        self.who.guild = Some(guild);
        self
    }

    /// Runs in `channel` (default: the guild's first text channel).
    pub fn in_channel(mut self, channel: Id<ChannelMarker>) -> Self {
        self.who.channel = Some(channel);
        self
    }

    /// Sets another, already filled option.
    pub fn option(mut self, name: &str, value: impl Into<TestOption>) -> Self {
        self.options.push((name.to_owned(), value.into()));
        self
    }

    /// Sends the autocomplete interaction and waits for its pipeline to
    /// end.
    ///
    /// # Errors
    ///
    /// [`TestError::UnknownCommand`], [`TestError::UnknownOption`],
    /// [`TestError::NotAutocomplete`] and the option checks before anything
    /// is sent; then [`TestError::Stopped`] or [`TestError::Timeout`].
    pub async fn run(self) -> Result<Response, TestError> {
        let ctx = self.app.resolve(&self.who)?;
        let data = command::chat_input(
            self.app.definitions(),
            &ChatInput {
                path: &self.path,
                options: &self.options,
                focus: Some((&self.focused, &self.typed)),
            },
            self.app.seeds(),
            &ctx,
        )?;
        self.app
            .execute(Dispatch {
                kind: 4,
                data,
                ctx,
                source: None,
            })
            .await
    }
}
