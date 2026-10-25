//! A test player: records every command.

use std::sync::{Arc, Mutex, PoisonError};

use super::{Cmd, Player};
use crate::track::TrackInfo;

/// The commands a [`FakePlayer`] received, shared with the test.
#[derive(Debug, Default, Clone)]
pub(crate) struct Played(Arc<Mutex<Vec<Cmd>>>);

impl Played {
    fn push(&self, cmd: Cmd) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(cmd);
    }

    /// Every command so far.
    pub(crate) fn all(&self) -> Vec<Cmd> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The token of the last `Play`.
    pub(crate) fn last_token(&self) -> Option<u64> {
        self.all().iter().rev().find_map(|cmd| match cmd {
            Cmd::Play(token, ..) => Some(*token),
            _ => None,
        })
    }

    /// The titles of every `Play`, in order.
    pub(crate) fn titles(&self) -> Vec<String> {
        self.all()
            .iter()
            .filter_map(|cmd| match cmd {
                Cmd::Play(_, info, _) => Some(info.title().unwrap_or_default().to_owned()),
                _ => None,
            })
            .collect()
    }
}

/// A player that records its commands.
#[derive(Debug, Default)]
pub(crate) struct FakePlayer(pub(crate) Played);

impl Player for FakePlayer {
    fn play(&self, token: u64, track: &TrackInfo, volume: f32) {
        self.0.push(Cmd::Play(token, track.clone(), volume));
    }

    fn pause(&self, token: u64) {
        self.0.push(Cmd::Pause(token));
    }

    fn resume(&self, token: u64) {
        self.0.push(Cmd::Resume(token));
    }

    fn stop(&self, token: u64) {
        self.0.push(Cmd::Stop(token));
    }

    fn set_volume(&self, token: u64, volume: f32) {
        self.0.push(Cmd::SetVolume(token, volume));
    }
}
