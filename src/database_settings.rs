//! Database-only settings in Rakazo (spec §7): detect what exists, decide what to fill,
//! fill it through Rakazo's own code paths, and record what was filled.

use serde::{Deserialize, Serialize};

use crate::answers::{FrontEnd, OWNER_PASSWORD_VAR, PASSWORD_TRIES, Password, PasswordUse};
use crate::checklist::Checklist;
use crate::lemonade::{RELAY_ORIGIN, RELAY_URL, SPEECH_MODEL, TRANSCRIPTION_MODEL};
use crate::memory::{CONNECT_STEPS, ConnectedSpace};
use crate::model_provider::{Choice, OwnerModel, Provider};
use crate::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseSetting {
    Owner,
    /// The Owner's voice credential, pointing at the relay.
    Voice,
    /// The default Space's memory connection.
    Memory,
    /// The Owner's connection to the chosen Model provider, for OpenAI and Gemini.
    ModelConnection,
}

/// Why the Owner's model connection still has to take the saved key. It stays in
/// `settings.json` until `models.connect` succeeds, so a failed fill-in is tried again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Due {
    NewKey,
    /// The Instance switched to this provider, so its connection becomes the main model.
    NewProvider,
}

/// How `models.connect` saves the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connect {
    /// A new connection, which becomes the Owner's main model.
    Add,
    /// The key of the connection with this id.
    Update { id: String, make_default: bool },
}

impl Connect {
    pub fn make_default(&self) -> bool {
        match self {
            Self::Add => true,
            Self::Update { make_default, .. } => *make_default,
        }
    }
}

/// Notes what this run's Model provider and key mean for the Owner's connections. Call it
/// before `settings.model_provider` changes. `new_key` is whether the key differs from the
/// saved one.
pub fn note_model_change(settings: &mut Settings, chosen: Choice, new_key: bool) {
    let previous = settings.model_provider.filter(|was| *was != chosen);
    if let Some(was) = previous {
        // A connection to another provider is a different setting, never filled so far.
        settings
            .filled_by_setup
            .retain(|setting| *setting != DatabaseSetting::ModelConnection);
        if was.0.is_some_and(Provider::is_per_user) && !settings.model_providers_left.contains(&was)
        {
            settings.model_providers_left.push(was);
        }
    }
    settings.model_providers_left.retain(|left| *left != chosen);
    settings.model_connection_due = if !chosen.0.is_some_and(Provider::is_per_user) {
        None
    } else if previous.is_some() {
        Some(Due::NewProvider)
    } else if new_key {
        settings.model_connection_due.or(Some(Due::NewKey))
    } else {
        settings.model_connection_due
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Fill,
    /// Setup filled it before and it's gone: someone removed it in Rakazo.
    RemovedInRakazo,
    Leave,
}

pub fn decide(present: bool, filled_before: bool) -> Decision {
    match (present, filled_before) {
        (true, _) => Decision::Leave,
        (false, false) => Decision::Fill,
        (false, true) => Decision::RemovedInRakazo,
    }
}

/// A signed-in session. Its token is a bearer credential: it is never printed, and
/// signing out consumes it.
pub struct Session {
    pub(crate) token: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Session(hidden)")
    }
}

/// The Owner's default Space: the one Rakazo opens first.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DefaultSpace {
    pub id: String,
    /// Whether the Space has a memory connection, to any provider.
    pub memory: bool,
}

/// The Owner's self-hosted voice connection, as `voice.credentials` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConnection {
    pub is_default: bool,
    pub voice_id: String,
    pub endpoint: String,
    pub stt_endpoint: String,
    pub stt_model: String,
}

impl VoiceConnection {
    /// Setup's own connection as it was made on another OS: the voice in use, transcribing
    /// through the relay with `model`. Anything else is the Self-hoster's own choice.
    pub fn is_setups_with(&self, model: &str) -> bool {
        self.is_default
            && self.endpoint == RELAY_URL
            && self.stt_endpoint.is_empty()
            && self.stt_model == model
    }
}

/// Why sign-in failed. Each holds the message for the checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInError {
    /// HTTP 401: Rakazo refused the email and password.
    Rejected(String),
    Failed(String),
}

impl std::fmt::Display for SignInError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(message) | Self::Failed(message) => f.write_str(message),
        }
    }
}

/// The running Instance's Rakazo, as the fill-in sequence uses it.
pub trait Rakazo {
    /// Read-only: a verified user with this email and a password (`credential`) account.
    fn owner_exists(&self, email: &str) -> Result<bool, String>;
    /// Read-only: any voice credential for the user with this email.
    fn voice_exists(&self, email: &str) -> Result<bool, String>;
    /// Read-only: the id of the user's connection to the provider, setup's own first, or
    /// else the newest.
    fn model_connection(&self, email: &str, provider: Provider) -> Result<Option<String>, String>;
    /// Read-only. `None` when the user has no default Space.
    fn default_space(&self, email: &str) -> Result<Option<DefaultSpace>, String>;
    /// Read-only: every Space connected to Hindsight, whoever connected it.
    fn spaces_on_hindsight(&self) -> Result<Vec<ConnectedSpace>, String>;
    fn bootstrap_owner(&self, email: &str, password: &Password) -> Result<(), String>;
    fn sign_in(&self, email: &str, password: &Password) -> Result<Session, SignInError>;
    /// `POST /rpc/voice/connect` with the relay, `kokoro-v1` and this OS's Whisper model.
    /// It updates the Owner's self-hosted connection when there is one; `voice_id` keeps
    /// the voice, which Rakazo otherwise sets to the server's first.
    fn connect_voice(&self, session: &Session, voice_id: Option<&str>) -> Result<(), String>;
    /// `POST /rpc/voice/credentials`: the self-hosted connection `connect_voice` would update.
    fn voice_connection(&self, session: &Session) -> Result<Option<VoiceConnection>, String>;
    /// `POST /rpc/models/connect` with the key.
    fn connect_model(
        &self,
        session: &Session,
        model: OwnerModel,
        connect: &Connect,
    ) -> Result<(), String>;
    /// Connects the Space to Hindsight with the tenant key, leaving Rakazo's memory
    /// toggles at their defaults.
    fn connect_memory(
        &self,
        session: &Session,
        space_id: &str,
        tenant_key: &str,
    ) -> Result<(), String>;
    fn sign_out(&self, session: Session) -> Result<(), String>;
}

/// The Owner's model connection, when the Model provider is one Rakazo connects per user.
#[derive(Debug)]
pub struct ModelFill<'a> {
    pub model: OwnerModel<'a>,
    /// Cleared once the connection has the key.
    pub due: &'a mut Option<Due>,
}

/// The settings this run may fill besides the Owner.
#[derive(Debug, Default)]
pub struct Fillable<'a> {
    /// The voice models are there, so voice is a setting to fill.
    pub voice: bool,
    /// `settings.json`'s `voice_model_due`, cleared once the Owner's connection is off it.
    pub voice_model_due: Option<&'a mut Option<String>>,
    /// Hindsight's tenant key, when memory is on.
    pub memory: Option<&'a str>,
    pub model: Option<ModelFill<'a>>,
}

/// Steps 2–12 of the sequence. Every failure becomes a Checklist item; none stops setup.
/// `filled` is what `settings.json` records as filled by setup, and gains what this run fills.
pub fn fill_in(
    rakazo: &dyn Rakazo,
    front: &mut dyn FrontEnd,
    email: &str,
    password: Option<Password>,
    mut fillable: Fillable,
    filled: &mut Vec<DatabaseSetting>,
    checklist: &mut Checklist,
) {
    let owner_present = match rakazo.owner_exists(email) {
        Ok(present) => present,
        Err(err) => {
            checklist.add(format!(
                "Setup couldn't check Rakazo's database for the Owner account: {err}. Run `rakazo-stack setup` again."
            ));
            return;
        }
    };
    let fill_owner = wanted(
        front,
        decide(owner_present, filled.contains(&DatabaseSetting::Owner)),
        &format!("The Owner account {email} was removed in Rakazo. Add it back?"),
    );
    if !owner_present && !fill_owner {
        return;
    }
    // A new Owner account has no voice yet, whatever an earlier one had.
    let voice_present = if !fillable.voice {
        None
    } else if fill_owner {
        Some(false)
    } else {
        match rakazo.voice_exists(email) {
            Ok(present) => Some(present),
            Err(err) => {
                checklist.add(voice_not_connected(&format!(
                    "setup couldn't check Rakazo's database for it ({err})"
                )));
                None
            }
        }
    };
    let voice = match voice_present {
        None => Decision::Leave,
        Some(_) if fill_owner => Decision::Fill,
        Some(present) => decide(present, filled.contains(&DatabaseSetting::Voice)),
    };
    // A restore from the other side of the macOS line (spec §16). A connection made on
    // this run already has this OS's model.
    let voice_model = match (voice_present, fillable.voice_model_due.take()) {
        (Some(true), Some(due)) => due.clone().map(|model| (model, due)),
        (Some(false), Some(due)) => {
            *due = None;
            None
        }
        _ => None,
    };
    let fill_voice = wanted(
        front,
        voice,
        "Your voice settings were removed in Rakazo. Add them back?",
    );
    let model = fillable.model.as_ref().map(|fill| fill.model);
    let connect = match &fillable.model {
        // A new Owner account has no model connection either.
        Some(_) if fill_owner => Some(Connect::Add),
        Some(fill) => model_to_connect(rakazo, front, email, fill, filled, checklist),
        None => None,
    };
    // A new Owner's default Space appears with the Owner, so it is looked up afterwards.
    let mut memory_space = match fillable.memory {
        Some(_) if owner_present => memory_to_fill(rakazo, front, email, filled, checklist),
        _ => None,
    };
    let fill_memory = memory_space.is_some() || (fill_owner && fillable.memory.is_some());
    if !fill_owner && !fill_voice && !fill_memory && connect.is_none() && voice_model.is_none() {
        return;
    }

    let not_filled = |reason: &str, checklist: &mut Checklist| {
        let reason = reason.trim_end_matches('.');
        if fill_owner {
            checklist.add(owner_not_created(reason));
        }
        if fill_voice {
            checklist.add(voice_not_connected(reason));
        }
        if let Some((stale, _)) = &voice_model {
            checklist.add(voice_model_not_changed(stale, reason));
        }
        if let (Some(model), Some(connect)) = (model, &connect) {
            checklist.add(model_not_connected(model, connect, reason));
        }
        if fill_memory {
            checklist.add(memory_not_connected(reason));
        }
    };
    let asked = password.is_none();
    let used = if fill_owner {
        PasswordUse::NewOwner
    } else {
        PasswordUse::SignIn
    };
    let mut password =
        match password.map_or_else(|| front.password_for_fill_in(used), |p| Ok(Some(p))) {
            Ok(Some(password)) => password,
            Ok(None) => {
                not_filled(
                    &format!("no Owner password was given (set {OWNER_PASSWORD_VAR})"),
                    checklist,
                );
                return;
            }
            Err(err) => {
                not_filled(&err.to_string(), checklist);
                return;
            }
        };
    if fill_owner {
        if let Err(err) = rakazo.bootstrap_owner(email, &password) {
            not_filled(&err, checklist);
            return;
        }
        record(filled, DatabaseSetting::Owner);
        if fillable.memory.is_some() {
            memory_space = memory_to_fill(rakazo, front, email, filled, checklist);
        }
    }

    let mut tries_left = PASSWORD_TRIES - 1;
    let session = loop {
        let err = match rakazo.sign_in(email, &password) {
            Ok(session) => break session,
            Err(SignInError::Rejected(rejected)) if asked && !fill_owner && tries_left > 0 => {
                match front.password_for_fill_in(PasswordUse::SignInAgain { tries_left }) {
                    Ok(Some(again)) => {
                        password = again;
                        tries_left -= 1;
                        continue;
                    }
                    Ok(None) => rejected,
                    Err(err) => err.to_string(),
                }
            }
            Err(err) => err.to_string(),
        };
        if fill_owner {
            checklist.add(format!(
                "Setup created the Owner account {email} but couldn't sign in with it: {err}. Sign in to Rakazo to check the email and password."
            ));
        }
        let reason = format!("setup couldn't sign in as the Owner ({err})");
        if fill_voice {
            checklist.add(voice_not_connected(&reason));
        }
        if let Some((stale, _)) = &voice_model {
            checklist.add(voice_model_not_changed(stale, &reason));
        }
        if let (Some(model), Some(connect)) = (model, &connect) {
            checklist.add(model_not_connected(model, connect, &reason));
        }
        if memory_space.is_some() {
            checklist.add(memory_not_connected(&reason));
        }
        return;
    };
    if fill_voice {
        match rakazo.connect_voice(&session, None) {
            Ok(()) => {
                record(filled, DatabaseSetting::Voice);
                checklist.add(voice_consent());
            }
            Err(err) => checklist.add(voice_not_connected(&err)),
        }
    }
    if let Some((stale, due)) = voice_model {
        let moved = match rakazo.voice_connection(&session) {
            Ok(Some(connection)) if connection.is_setups_with(&stale) => {
                rakazo.connect_voice(&session, Some(&connection.voice_id))
            }
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        };
        match moved {
            Ok(()) => *due = None,
            Err(err) => checklist.add(voice_model_not_changed(&stale, &err)),
        }
    }
    if let (Some(fill), Some(connect)) = (fillable.model.as_mut(), connect) {
        match rakazo.connect_model(&session, fill.model, &connect) {
            Ok(()) => {
                record(filled, DatabaseSetting::ModelConnection);
                *fill.due = None;
            }
            Err(err) => checklist.add(model_not_connected(fill.model, &connect, &err)),
        }
    }
    if let (Some(space), Some(key)) = (memory_space, fillable.memory) {
        match rakazo.connect_memory(&session, &space, key) {
            Ok(()) => record(filled, DatabaseSetting::Memory),
            Err(err) => checklist.add(memory_not_connected(&err)),
        }
    }
    if let Err(err) = rakazo.sign_out(session) {
        checklist.add(format!(
            "Setup couldn't sign out of Rakazo: {err}. Its session ends on its own; the Owner can also sign out other sessions in Rakazo's settings."
        ));
    }
}

fn wanted(front: &mut dyn FrontEnd, decision: Decision, question: &str) -> bool {
    match decision {
        Decision::Fill => true,
        Decision::RemovedInRakazo => front.add_back(question),
        Decision::Leave => false,
    }
}

fn record(filled: &mut Vec<DatabaseSetting>, setting: DatabaseSetting) {
    if !filled.contains(&setting) {
        filled.push(setting);
    }
}

/// How the Owner's model connection will be saved, if it will be.
fn model_to_connect(
    rakazo: &dyn Rakazo,
    front: &mut dyn FrontEnd,
    email: &str,
    fill: &ModelFill,
    filled: &[DatabaseSetting],
    checklist: &mut Checklist,
) -> Option<Connect> {
    let provider = fill.model.provider;
    match rakazo.model_connection(email, provider) {
        Ok(Some(id)) => fill.due.map(|due| Connect::Update {
            id,
            make_default: due == Due::NewProvider,
        }),
        Ok(None) => wanted(
            front,
            decide(false, filled.contains(&DatabaseSetting::ModelConnection)),
            &format!(
                "Your {} model connection was removed in Rakazo. Add it back?",
                provider.name()
            ),
        )
        .then_some(Connect::Add),
        Err(err) => {
            checklist.add(model_not_connected(
                fill.model,
                &Connect::Add,
                &format!("setup couldn't check Rakazo's database for it ({err})"),
            ));
            None
        }
    }
}

/// The default Space's id when its memory will be filled.
fn memory_to_fill(
    rakazo: &dyn Rakazo,
    front: &mut dyn FrontEnd,
    email: &str,
    filled: &[DatabaseSetting],
    checklist: &mut Checklist,
) -> Option<String> {
    let space = match rakazo.default_space(email) {
        Ok(Some(space)) => space,
        Ok(None) => {
            checklist.add(memory_not_connected(&format!(
                "Rakazo has no default Space for {email}"
            )));
            return None;
        }
        Err(err) => {
            checklist.add(memory_not_connected(&format!(
                "setup couldn't read the default Space from Rakazo's database: {err}"
            )));
            return None;
        }
    };
    wanted(
        front,
        decide(space.memory, filled.contains(&DatabaseSetting::Memory)),
        "The default Space's memory was removed in Rakazo. Add it back?",
    )
    .then_some(space.id)
}

/// With memory off, one item for each Space still connected to Hindsight (spec §7).
pub fn still_connected(rakazo: &dyn Rakazo, checklist: &mut Checklist) {
    match rakazo.spaces_on_hindsight() {
        Ok(spaces) => {
            for space in &spaces {
                checklist.add(crate::memory::still_connected_item(space));
            }
        }
        Err(err) => checklist.add(format!(
            "Setup couldn't check which Spaces are still connected to Hindsight: {err}. Each Space's owner checks Settings → Memory and disconnects Hindsight there."
        )),
    }
}

/// One item for each provider the Instance left whose connection the Owner still has
/// (spec §7). A provider without one is forgotten.
pub fn old_connections(
    rakazo: &dyn Rakazo,
    email: &str,
    chosen: Choice,
    left: &mut Vec<Choice>,
    checklist: &mut Checklist,
) {
    left.retain(|choice| {
        let Some(provider) = choice.0 else {
            return false;
        };
        let name = provider.name();
        match rakazo.model_connection(email, provider) {
            Ok(None) => false,
            Ok(Some(_)) => {
                // A per-user provider's new connection takes over as the main model.
                let why = if chosen.0.is_some_and(Provider::is_per_user) {
                    ""
                } else {
                    ", because until then it stays the Owner's default"
                };
                checklist.add(format!(
                    "You switched away from {name}: remove the old connection in Settings → Models{why}."
                ));
                true
            }
            Err(err) => {
                checklist.add(format!(
                    "Setup couldn't check whether the Owner still has a {name} connection: {err}. If so, remove it in Settings → Models."
                ));
                true
            }
        }
    });
}

fn model_not_connected(model: OwnerModel, connect: &Connect, reason: &str) -> String {
    let name = model.provider.name();
    let in_rakazo = model.provider.rakazo_name();
    if connect.make_default() {
        format!(
            "{name} chat wasn't connected for the Owner: {reason}. Run `rakazo-stack setup` again, or in Rakazo open Settings → Models, connect {in_rakazo} with your API key and choose the model {}.",
            model.model
        )
    } else {
        format!(
            "The Owner's {name} connection didn't get the new key: {reason}. Run `rakazo-stack setup` again, or in Rakazo open Settings → Models and enter the key on the {in_rakazo} connection."
        )
    }
}

fn owner_not_created(reason: &str) -> String {
    format!(
        "The Owner account wasn't created: {reason}. Run `rakazo-stack setup` again; if it keeps failing, create it as Rakazo's docs describe under \"Installations without email\"."
    )
}

/// Rakazo's web app asks each person once before it first sends audio to a voice server
/// (spec §9). It is the person's own answer, so setup only says what the question means.
fn voice_consent() -> String {
    format!(
        "The first time you use voice, Rakazo asks \"Share voice data with Self-hosted ({RELAY_ORIGIN})?\". Choose Allow: that server is this Instance's own Lemonade, so the audio stays on this machine."
    )
}

fn voice_model_not_changed(stale: &str, reason: &str) -> String {
    format!(
        "The Owner's voice may still transcribe with {stale}, the Whisper model of the OS the Backup was made on, which this machine doesn't download: {reason}. Run `rakazo-stack setup` again, or in Rakazo open Settings → Voice and change the Transcription model to {TRANSCRIPTION_MODEL}."
    )
}

fn voice_not_connected(reason: &str) -> String {
    format!(
        "Voice wasn't connected for the Owner: {reason}. Run `rakazo-stack setup` again, or in Rakazo open Settings → Voice, pick Self-hosted, and enter the Server URL {RELAY_URL}, the Speech model {SPEECH_MODEL} and the Transcription model {TRANSCRIPTION_MODEL}."
    )
}

fn memory_not_connected(reason: &str) -> String {
    format!(
        "The default Space wasn't connected to memory: {reason}. Run `rakazo-stack setup` again, or connect it in Rakazo: {CONNECT_STEPS}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{AccessMode, AuthKey, AuthKeyState};
    use crate::answers::{InputError, Prompt, Wizard};
    use crate::secrets::PastedKey;
    use std::cell::RefCell;

    const EMAIL: &str = "owner@example.com";
    const TENANT_KEY: &str = "placeholder-tenant-key";

    fn password() -> Password {
        Password::new("correct-horse-battery".into()).unwrap()
    }

    #[derive(Default)]
    struct FakeRakazo {
        owner: RefCell<bool>,
        voice: RefCell<bool>,
        memory: RefCell<bool>,
        bootstrap_error: Option<String>,
        sign_in_error: Option<String>,
        /// How many sign-ins in a row Rakazo rejects with HTTP 401.
        rejections: RefCell<usize>,
        connect_error: Option<String>,
        /// What `voice.credentials` lists, and the voice each `voice.connect` kept.
        voice_connection: Option<VoiceConnection>,
        credentials_error: Option<String>,
        voice_ids: RefCell<Vec<Option<String>>>,
        memory_error: Option<String>,
        /// The providers the Owner has a connection to.
        connections: RefCell<Vec<Provider>>,
        model_error: Option<String>,
        connects: RefCell<Vec<(&'static str, String, Connect)>>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl Rakazo for FakeRakazo {
        fn owner_exists(&self, _email: &str) -> Result<bool, String> {
            self.calls.borrow_mut().push("detect");
            Ok(*self.owner.borrow())
        }
        fn voice_exists(&self, _email: &str) -> Result<bool, String> {
            self.calls.borrow_mut().push("detect voice");
            Ok(*self.voice.borrow())
        }
        fn connect_voice(&self, session: &Session, voice_id: Option<&str>) -> Result<(), String> {
            assert_eq!(session.token, "placeholder-token");
            self.calls.borrow_mut().push("connect voice");
            self.voice_ids
                .borrow_mut()
                .push(voice_id.map(str::to_string));
            match &self.connect_error {
                Some(err) => Err(err.clone()),
                None => {
                    *self.voice.borrow_mut() = true;
                    Ok(())
                }
            }
        }
        fn voice_connection(&self, session: &Session) -> Result<Option<VoiceConnection>, String> {
            assert_eq!(session.token, "placeholder-token");
            self.calls.borrow_mut().push("voice credentials");
            match &self.credentials_error {
                Some(err) => Err(err.clone()),
                None => Ok(self.voice_connection.clone()),
            }
        }
        fn model_connection(&self, _: &str, provider: Provider) -> Result<Option<String>, String> {
            self.calls.borrow_mut().push("detect model");
            Ok(self
                .connections
                .borrow()
                .contains(&provider)
                .then(|| format!("{}-connection", provider.id())))
        }
        fn connect_model(
            &self,
            session: &Session,
            model: OwnerModel,
            connect: &Connect,
        ) -> Result<(), String> {
            assert_eq!(session.token, "placeholder-token");
            self.calls.borrow_mut().push("connect model");
            self.connects.borrow_mut().push((
                model.provider.id(),
                model.model.to_string(),
                connect.clone(),
            ));
            match &self.model_error {
                Some(err) => Err(err.clone()),
                None => {
                    let mut connections = self.connections.borrow_mut();
                    if !connections.contains(&model.provider) {
                        connections.push(model.provider);
                    }
                    Ok(())
                }
            }
        }
        fn default_space(&self, _email: &str) -> Result<Option<DefaultSpace>, String> {
            self.calls.borrow_mut().push("default-space");
            Ok(self.owner.borrow().then(|| DefaultSpace {
                id: "space-default".into(),
                memory: *self.memory.borrow(),
            }))
        }
        fn spaces_on_hindsight(&self) -> Result<Vec<ConnectedSpace>, String> {
            Ok(vec![
                ConnectedSpace {
                    space: "Personal".into(),
                    by: Some(EMAIL.into()),
                },
                ConnectedSpace {
                    space: "Team".into(),
                    by: None,
                },
            ])
        }
        fn connect_memory(
            &self,
            session: &Session,
            space_id: &str,
            tenant_key: &str,
        ) -> Result<(), String> {
            assert_eq!(session.token, "placeholder-token");
            assert_eq!((space_id, tenant_key), ("space-default", TENANT_KEY));
            self.calls.borrow_mut().push("connect-memory");
            match &self.memory_error {
                Some(err) => Err(err.clone()),
                None => {
                    *self.memory.borrow_mut() = true;
                    Ok(())
                }
            }
        }
        fn bootstrap_owner(&self, _email: &str, _password: &Password) -> Result<(), String> {
            self.calls.borrow_mut().push("bootstrap-owner");
            match &self.bootstrap_error {
                Some(err) => Err(err.clone()),
                None => {
                    *self.owner.borrow_mut() = true;
                    Ok(())
                }
            }
        }
        fn sign_in(&self, _email: &str, _password: &Password) -> Result<Session, SignInError> {
            self.calls.borrow_mut().push("sign-in");
            if *self.rejections.borrow() > 0 {
                *self.rejections.borrow_mut() -= 1;
                return Err(SignInError::Rejected(
                    "Invalid email or password (HTTP 401)".into(),
                ));
            }
            match &self.sign_in_error {
                Some(err) => Err(SignInError::Failed(err.clone())),
                None => Ok(Session {
                    token: "placeholder-token".into(),
                }),
            }
        }
        fn sign_out(&self, session: Session) -> Result<(), String> {
            assert_eq!(session.token, "placeholder-token");
            self.calls.borrow_mut().push("sign-out");
            Ok(())
        }
    }

    /// A front end that must not be asked anything.
    struct Silent;

    impl FrontEnd for Silent {
        fn owner_email(&mut self, _: Option<&str>) -> Result<String, InputError> {
            panic!("asked for the email")
        }
        fn password_now(&mut self, _: bool) -> Result<Option<Password>, InputError> {
            panic!("asked for the password")
        }
        fn password_for_fill_in(&mut self, _: PasswordUse) -> Result<Option<Password>, InputError> {
            panic!("asked for the password")
        }
        fn model_provider(&mut self, _: Option<Choice>) -> Result<Choice, InputError> {
            panic!("asked for the Model provider")
        }
        fn model_key(&mut self, _: Provider, _: bool) -> Result<Option<PastedKey>, InputError> {
            panic!("asked for a key")
        }
        fn composio_key(&mut self, _: bool) -> Result<Option<PastedKey>, InputError> {
            panic!("asked for the Composio key")
        }
        fn check_zdr_again(&mut self) -> bool {
            panic!("asked about ZDR only")
        }
        fn memory(
            &mut self,
            _: Option<crate::memory::Memory>,
        ) -> Result<crate::memory::Memory, InputError> {
            panic!("asked about memory")
        }
        fn add_back(&mut self, _: &str) -> bool {
            panic!("asked to add back")
        }
        fn access(&mut self, _: AccessMode) -> Result<String, InputError> {
            panic!("asked for the access mode")
        }
        fn device_name(&mut self, _: &str) -> Result<String, InputError> {
            panic!("asked for the device name")
        }
        fn auth_key(&mut self, _: AuthKeyState) -> Result<Option<AuthKey>, InputError> {
            panic!("asked for the auth key")
        }
        fn instance_url(&mut self, _: Option<&str>) -> Result<String, InputError> {
            panic!("asked for the Instance URL")
        }
        fn offers_instance_url(&self) -> bool {
            panic!("asked about the Instance URL")
        }
        fn confirm_switch(&mut self, _: &str) -> bool {
            panic!("asked to switch")
        }
    }

    struct Answers(Vec<&'static str>);

    impl Prompt for Answers {
        fn line(&mut self, _: &str) -> Option<String> {
            (!self.0.is_empty()).then(|| self.0.remove(0).to_string())
        }
        fn secret(&mut self, question: &str) -> Option<String> {
            self.line(question)
        }
    }

    #[test]
    fn decides_from_presence_and_the_record() {
        assert_eq!(decide(true, true), Decision::Leave);
        assert_eq!(decide(true, false), Decision::Leave);
        assert_eq!(decide(false, false), Decision::Fill);
        assert_eq!(decide(false, true), Decision::RemovedInRakazo);
    }

    #[test]
    fn a_first_run_creates_the_owner_signs_in_and_out_and_records_it() {
        let rakazo = FakeRakazo::default();
        let mut filled = Vec::new();
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            Some(password()),
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(
            *rakazo.calls.borrow(),
            ["detect", "bootstrap-owner", "sign-in", "sign-out"]
        );
        assert_eq!(filled, [DatabaseSetting::Owner]);
        assert_eq!(checklist.items().count(), 1, "only the standing item");
    }

    #[test]
    fn an_owner_already_there_needs_no_password_and_no_bootstrap() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            None,
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);
        assert_eq!(filled, [DatabaseSetting::Owner]);
    }

    #[test]
    fn a_removed_owner_comes_back_only_when_the_wizard_says_yes() {
        let rakazo = FakeRakazo::default();
        let mut filled = vec![DatabaseSetting::Owner];
        let mut checklist = Checklist::default();
        let mut no = Wizard {
            prompt: Answers(vec![""]),
        };
        fill_in(
            &rakazo,
            &mut no,
            EMAIL,
            None,
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);

        let mut yes = Wizard {
            prompt: Answers(vec!["y", "correct-horse-battery", "correct-horse-battery"]),
        };
        fill_in(
            &rakazo,
            &mut yes,
            EMAIL,
            None,
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(
            *rakazo.calls.borrow(),
            ["detect", "detect", "bootstrap-owner", "sign-in", "sign-out"]
        );
    }

    #[test]
    fn a_scripted_run_without_a_password_adds_an_item() {
        let rakazo = FakeRakazo::default();
        let mut filled = Vec::new();
        let mut checklist = Checklist::default();
        let mut front = crate::answers::NoQuestions {
            env: |_: &str| None,
        };
        fill_in(
            &rakazo,
            &mut front,
            EMAIL,
            None,
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);
        assert!(filled.is_empty());
        assert!(
            checklist
                .items()
                .next()
                .unwrap()
                .contains(OWNER_PASSWORD_VAR)
        );
    }

    #[test]
    fn a_reason_ending_in_a_full_stop_reads_as_one_sentence() {
        let rakazo = FakeRakazo {
            bootstrap_error: Some("the two Owner passwords don't match.".into()),
            ..Default::default()
        };
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            Some(password()),
            Fillable::default(),
            &mut Vec::new(),
            &mut checklist,
        );
        let item = checklist.items().next().unwrap();
        assert!(item.contains("don't match. Run"), "{item}");
    }

    #[test]
    fn failures_become_checklist_items_and_setup_carries_on() {
        let rakazo = FakeRakazo {
            bootstrap_error: Some("Existing account password does not match".into()),
            ..Default::default()
        };
        let mut filled = Vec::new();
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            Some(password()),
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert!(filled.is_empty());
        assert!(
            checklist
                .items()
                .next()
                .unwrap()
                .contains("Existing account password does not match")
        );

        let rakazo = FakeRakazo {
            sign_in_error: Some("Invalid email or password".into()),
            ..Default::default()
        };
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            Some(password()),
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(filled, [DatabaseSetting::Owner]);
        assert!(
            checklist
                .items()
                .next()
                .unwrap()
                .contains("Invalid email or password")
        );
    }

    fn run(
        rakazo: &FakeRakazo,
        front: &mut dyn FrontEnd,
        password: Option<Password>,
        filled: &mut Vec<DatabaseSetting>,
    ) -> Vec<String> {
        let mut checklist = Checklist::default();
        let fillable = Fillable {
            voice: true,
            ..Default::default()
        };
        fill_in(
            rakazo,
            front,
            EMAIL,
            password,
            fillable,
            filled,
            &mut checklist,
        );
        checklist.items().map(str::to_string).collect()
    }

    #[test]
    fn a_first_run_connects_voice_in_the_same_session() {
        let rakazo = FakeRakazo::default();
        let mut filled = Vec::new();
        let items = run(&rakazo, &mut Silent, Some(password()), &mut filled);
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "bootstrap-owner",
                "sign-in",
                "connect voice",
                "sign-out"
            ]
        );
        assert_eq!(filled, [DatabaseSetting::Owner, DatabaseSetting::Voice]);
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(
            items[0].contains("Share voice data with Self-hosted (http://lemonade-relay:13306)?")
        );
    }

    #[test]
    fn a_rejected_password_is_asked_again_up_to_three_times() {
        const TYPED: &str = "correct-horse-battery";
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            rejections: RefCell::new(2),
            ..Default::default()
        };
        let mut wizard = Wizard {
            prompt: Answers(vec![TYPED, TYPED, TYPED]),
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let items = run(&rakazo, &mut wizard, None, &mut filled);
        assert_eq!(
            rakazo.calls.borrow()[1..],
            [
                "detect voice",
                "sign-in",
                "sign-in",
                "sign-in",
                "connect voice",
                "sign-out"
            ]
        );
        assert_eq!(filled, [DatabaseSetting::Owner, DatabaseSetting::Voice]);
        assert!(
            items.iter().all(|item| !item.contains("sign in")),
            "{items:?}"
        );

        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            rejections: RefCell::new(3),
            ..Default::default()
        };
        let mut wizard = Wizard {
            prompt: Answers(vec![TYPED, TYPED, TYPED, TYPED]),
        };
        let items = run(
            &rakazo,
            &mut wizard,
            None,
            &mut vec![DatabaseSetting::Owner],
        );
        assert_eq!(
            rakazo
                .calls
                .borrow()
                .iter()
                .filter(|c| **c == "sign-in")
                .count(),
            3
        );
        assert_eq!(wizard.prompt.0, [TYPED]);
        assert!(
            items[0].contains("Invalid email or password (HTTP 401)"),
            "{items:?}"
        );
    }

    #[test]
    fn a_given_password_rakazo_rejects_is_not_asked_again() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            rejections: RefCell::new(1),
            ..Default::default()
        };
        let items = run(
            &rakazo,
            &mut Silent,
            Some(password()),
            &mut vec![DatabaseSetting::Owner],
        );
        assert_eq!(
            rakazo
                .calls
                .borrow()
                .iter()
                .filter(|c| **c == "sign-in")
                .count(),
            1
        );
        assert!(
            items[0].contains("Invalid email or password (HTTP 401)"),
            "{items:?}"
        );
    }

    #[test]
    fn a_rerun_with_voice_present_leaves_it_alone() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            voice: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner, DatabaseSetting::Voice];
        run(&rakazo, &mut Silent, None, &mut filled);
        assert_eq!(*rakazo.calls.borrow(), ["detect", "detect voice"]);
    }

    #[test]
    fn voice_never_set_is_filled_for_an_existing_owner() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        run(&rakazo, &mut Silent, Some(password()), &mut filled);
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "detect voice",
                "sign-in",
                "connect voice",
                "sign-out"
            ]
        );
        assert_eq!(filled, [DatabaseSetting::Owner, DatabaseSetting::Voice]);
    }

    #[test]
    fn removed_voice_asks_the_wizard_and_stays_removed_without_questions() {
        let removed = || FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let both = || vec![DatabaseSetting::Owner, DatabaseSetting::Voice];

        let rakazo = removed();
        let mut front = crate::answers::NoQuestions {
            env: |_: &str| Some("correct-horse-battery".to_string()),
        };
        let items = run(&rakazo, &mut front, Some(password()), &mut both());
        assert_eq!(*rakazo.calls.borrow(), ["detect", "detect voice"]);
        assert_eq!(items.len(), 1, "{items:?}");

        let rakazo = removed();
        let mut no = Wizard {
            prompt: Answers(vec!["n"]),
        };
        run(&rakazo, &mut no, None, &mut both());
        assert_eq!(*rakazo.calls.borrow(), ["detect", "detect voice"]);

        let rakazo = removed();
        let mut yes = Wizard {
            prompt: Answers(vec!["y", "correct-horse-battery", "correct-horse-battery"]),
        };
        run(&rakazo, &mut yes, None, &mut both());
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "detect voice",
                "sign-in",
                "connect voice",
                "sign-out"
            ]
        );
    }

    #[test]
    fn voice_not_ready_is_neither_detected_nor_filled() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            None,
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);
    }

    #[test]
    fn a_failed_voice_connect_becomes_an_item_with_the_ui_steps_and_still_signs_out() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            connect_error: Some("Couldn't reach the speech server (HTTP 400)".into()),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let items = run(&rakazo, &mut Silent, Some(password()), &mut filled);
        assert_eq!(rakazo.calls.borrow().last().unwrap(), &"sign-out");
        assert_eq!(filled, [DatabaseSetting::Owner]);
        assert!(items[0].contains("Couldn't reach the speech server (HTTP 400)"));
        assert!(items[0].contains("http://lemonade-relay:13306/api/v1"));
        assert!(items[0].contains(TRANSCRIPTION_MODEL));
    }

    #[test]
    fn voice_without_a_password_says_to_run_again_with_it() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut front = crate::answers::NoQuestions {
            env: |_: &str| None,
        };
        let items = run(&rakazo, &mut front, None, &mut vec![DatabaseSetting::Owner]);
        assert!(items[0].starts_with("Voice wasn't connected"), "{items:?}");
        assert!(items[0].contains(OWNER_PASSWORD_VAR));
    }

    const OTHER_OS_MODEL: &str = if cfg!(target_os = "macos") {
        "Whisper-Small"
    } else {
        "Whisper-Large-v3-Turbo"
    };

    fn made_on_another_os() -> VoiceConnection {
        VoiceConnection {
            is_default: true,
            voice_id: "af_bella".into(),
            endpoint: RELAY_URL.into(),
            stt_endpoint: String::new(),
            stt_model: OTHER_OS_MODEL.into(),
        }
    }

    /// A re-run after a restore whose Backup came from the other side of the macOS line.
    fn run_restored(
        rakazo: &FakeRakazo,
        front: &mut dyn FrontEnd,
        password: Option<Password>,
        due: &mut Option<String>,
    ) -> Vec<String> {
        let mut checklist = Checklist::default();
        fill_in(
            rakazo,
            front,
            EMAIL,
            password,
            Fillable {
                voice: true,
                voice_model_due: Some(due),
                ..Default::default()
            },
            &mut vec![DatabaseSetting::Owner, DatabaseSetting::Voice],
            &mut checklist,
        );
        checklist.items().map(str::to_string).collect()
    }

    fn restored(connection: VoiceConnection) -> FakeRakazo {
        FakeRakazo {
            owner: RefCell::new(true),
            voice: RefCell::new(true),
            voice_connection: Some(connection),
            ..Default::default()
        }
    }

    #[test]
    fn only_setups_connection_from_the_other_os_counts_as_made_there() {
        assert!(made_on_another_os().is_setups_with(OTHER_OS_MODEL));
        assert!(!made_on_another_os().is_setups_with(TRANSCRIPTION_MODEL));
        for changed in [
            VoiceConnection {
                is_default: false,
                ..made_on_another_os()
            },
            VoiceConnection {
                endpoint: "http://speech.example:8000/v1".into(),
                ..made_on_another_os()
            },
            VoiceConnection {
                stt_endpoint: "http://whisper.example:9000/v1".into(),
                ..made_on_another_os()
            },
            VoiceConnection {
                stt_model: "Whisper-Medium".into(),
                ..made_on_another_os()
            },
        ] {
            assert!(!changed.is_setups_with(OTHER_OS_MODEL), "{changed:?}");
        }
    }

    #[test]
    fn a_restore_from_the_other_os_moves_voice_to_this_oss_model_and_keeps_the_voice() {
        let rakazo = restored(made_on_another_os());
        let mut due = Some(OTHER_OS_MODEL.to_string());
        let items = run_restored(&rakazo, &mut Silent, Some(password()), &mut due);
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "detect voice",
                "sign-in",
                "voice credentials",
                "connect voice",
                "sign-out"
            ]
        );
        assert_eq!(*rakazo.voice_ids.borrow(), [Some("af_bella".to_string())]);
        assert_eq!(due, None);
        assert_eq!(items.len(), 1, "only the standing item: {items:?}");
    }

    #[test]
    fn a_connection_the_self_hoster_changed_is_left_alone_after_a_restore() {
        let rakazo = restored(VoiceConnection {
            stt_model: "Whisper-Medium".into(),
            ..made_on_another_os()
        });
        let mut due = Some(OTHER_OS_MODEL.to_string());
        run_restored(&rakazo, &mut Silent, Some(password()), &mut due);
        assert!(!rakazo.calls.borrow().contains(&"connect voice"));
        assert_eq!(due, None);

        let rakazo = FakeRakazo {
            voice_connection: None,
            ..restored(made_on_another_os())
        };
        let mut due = Some(OTHER_OS_MODEL.to_string());
        run_restored(&rakazo, &mut Silent, Some(password()), &mut due);
        assert!(!rakazo.calls.borrow().contains(&"connect voice"));
        assert_eq!(due, None);
    }

    #[test]
    fn without_a_password_the_move_waits_for_a_later_run() {
        let rakazo = restored(made_on_another_os());
        let mut front = crate::answers::NoQuestions {
            env: |_: &str| None,
        };
        let mut due = Some(OTHER_OS_MODEL.to_string());
        let items = run_restored(&rakazo, &mut front, None, &mut due);
        assert_eq!(*rakazo.calls.borrow(), ["detect", "detect voice"]);
        assert_eq!(due.as_deref(), Some(OTHER_OS_MODEL));
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(items[0].contains(OTHER_OS_MODEL), "{items:?}");
        assert!(items[0].contains(OWNER_PASSWORD_VAR), "{items:?}");
        assert!(
            items[0].contains(&format!(
                "change the Transcription model to {TRANSCRIPTION_MODEL}"
            )),
            "{items:?}"
        );
    }

    #[test]
    fn a_failed_move_keeps_it_due_and_still_signs_out() {
        for rakazo in [
            FakeRakazo {
                connect_error: Some("Couldn't reach the speech server (HTTP 400)".into()),
                ..restored(made_on_another_os())
            },
            FakeRakazo {
                credentials_error: Some("Couldn't reach the speech server (HTTP 400)".into()),
                ..restored(made_on_another_os())
            },
        ] {
            let mut due = Some(OTHER_OS_MODEL.to_string());
            let items = run_restored(&rakazo, &mut Silent, Some(password()), &mut due);
            assert_eq!(rakazo.calls.borrow().last().unwrap(), &"sign-out");
            assert_eq!(due.as_deref(), Some(OTHER_OS_MODEL));
            assert!(items[0].contains("(HTTP 400)"), "{items:?}");
        }
    }

    #[test]
    fn a_missing_connection_is_connected_afresh_and_nothing_stays_due() {
        let rakazo = FakeRakazo {
            voice: RefCell::new(false),
            ..restored(made_on_another_os())
        };
        let mut due = Some(OTHER_OS_MODEL.to_string());
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            Some(password()),
            Fillable {
                voice: true,
                voice_model_due: Some(&mut due),
                ..Default::default()
            },
            &mut vec![DatabaseSetting::Owner],
            &mut checklist,
        );
        assert_eq!(due, None);
        assert!(!rakazo.calls.borrow().contains(&"voice credentials"));
        assert_eq!(*rakazo.voice_ids.borrow(), [None]);
    }

    #[test]
    fn voice_not_ready_keeps_the_move_due() {
        let rakazo = restored(made_on_another_os());
        let mut due = Some(OTHER_OS_MODEL.to_string());
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            None,
            Fillable {
                voice_model_due: Some(&mut due),
                ..Default::default()
            },
            &mut vec![DatabaseSetting::Owner, DatabaseSetting::Voice],
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);
        assert_eq!(due.as_deref(), Some(OTHER_OS_MODEL));
    }

    fn fill_memory(
        rakazo: &FakeRakazo,
        front: &mut dyn FrontEnd,
        password: Option<Password>,
        filled: &mut Vec<DatabaseSetting>,
    ) -> Checklist {
        let mut checklist = Checklist::default();
        fill_in(
            rakazo,
            front,
            EMAIL,
            password,
            Fillable {
                memory: Some(TENANT_KEY),
                ..Default::default()
            },
            filled,
            &mut checklist,
        );
        checklist
    }

    #[test]
    fn a_first_run_with_memory_connects_the_new_owners_default_space() {
        let rakazo = FakeRakazo::default();
        let mut filled = Vec::new();
        let checklist = fill_memory(&rakazo, &mut Silent, Some(password()), &mut filled);
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "bootstrap-owner",
                "default-space",
                "sign-in",
                "connect-memory",
                "sign-out"
            ]
        );
        assert_eq!(filled, [DatabaseSetting::Owner, DatabaseSetting::Memory]);
        assert_eq!(checklist.items().count(), 1, "only the standing item");
    }

    #[test]
    fn memory_turned_on_later_needs_the_password_and_touches_nothing_else() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let mut scripted = crate::answers::NoQuestions {
            env: |_: &str| None,
        };
        let checklist = fill_memory(&rakazo, &mut scripted, None, &mut filled);
        assert_eq!(*rakazo.calls.borrow(), ["detect", "default-space"]);
        let item = checklist.items().next().unwrap();
        assert!(
            item.starts_with("The default Space wasn't connected to memory: no Owner password was given (set RAKAZO_STACK_OWNER_PASSWORD).")
                && item.contains("http://hindsight:8888"),
            "{item}"
        );

        rakazo.calls.borrow_mut().clear();
        fill_memory(&rakazo, &mut Silent, Some(password()), &mut filled);
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "default-space",
                "sign-in",
                "connect-memory",
                "sign-out"
            ]
        );
        assert_eq!(filled, [DatabaseSetting::Owner, DatabaseSetting::Memory]);

        rakazo.calls.borrow_mut().clear();
        let checklist = fill_memory(&rakazo, &mut Silent, None, &mut filled);
        assert_eq!(
            *rakazo.calls.borrow(),
            ["detect", "default-space"],
            "a connected Space is left alone, without asking for the password"
        );
        assert_eq!(checklist.items().count(), 1);
    }

    #[test]
    fn memory_removed_in_rakazo_comes_back_only_when_the_wizard_says_yes() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner, DatabaseSetting::Memory];
        let mut scripted = crate::answers::NoQuestions {
            env: |_: &str| None,
        };
        let checklist = fill_memory(&rakazo, &mut scripted, Some(password()), &mut filled);
        assert_eq!(*rakazo.calls.borrow(), ["detect", "default-space"]);
        assert_eq!(checklist.items().count(), 1);

        let mut yes = Wizard {
            prompt: Answers(vec!["y", "correct-horse-battery", "correct-horse-battery"]),
        };
        fill_memory(&rakazo, &mut yes, None, &mut filled);
        assert_eq!(
            rakazo.calls.borrow()[2..],
            [
                "detect",
                "default-space",
                "sign-in",
                "connect-memory",
                "sign-out"
            ]
        );
        assert_eq!(filled, [DatabaseSetting::Owner, DatabaseSetting::Memory]);
    }

    #[test]
    fn a_failed_connect_becomes_an_item_with_the_ui_steps_and_isnt_recorded() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            memory_error: Some("Hindsight rejected the API key (HTTP 400)".into()),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let checklist = fill_memory(&rakazo, &mut Silent, Some(password()), &mut filled);
        assert_eq!(filled, [DatabaseSetting::Owner]);
        let item = checklist.items().next().unwrap();
        assert!(
            item.starts_with("The default Space wasn't connected to memory: Hindsight rejected the API key (HTTP 400).")
                && item.contains("Settings → Memory → Connect memory provider → Hindsight → Advanced → Self-hosted"),
            "{item}"
        );
        assert!(rakazo.calls.borrow().ends_with(&["sign-out"]));
    }

    #[test]
    fn memory_off_never_looks_at_spaces_and_lists_those_still_connected() {
        let rakazo = FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        };
        let mut filled = vec![DatabaseSetting::Owner];
        let mut checklist = Checklist::default();
        fill_in(
            &rakazo,
            &mut Silent,
            EMAIL,
            Some(password()),
            Fillable::default(),
            &mut filled,
            &mut checklist,
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect"]);
        still_connected(&rakazo, &mut checklist);
        let items: Vec<_> = checklist.items().collect();
        assert_eq!(items.len(), 3);
        assert!(items[0].contains("\"Personal\", connected by owner@example.com,"));
        assert!(items[1].contains("\"Team\" is still connected"));
    }

    fn fill_model(
        rakazo: &FakeRakazo,
        front: &mut dyn FrontEnd,
        password: Option<Password>,
        provider: Provider,
        due: &mut Option<Due>,
        filled: &mut Vec<DatabaseSetting>,
    ) -> Vec<String> {
        let key = PastedKey::new("placeholder-key").unwrap();
        let model = OwnerModel::new(provider, &key, None).unwrap().unwrap();
        let mut checklist = Checklist::default();
        fill_in(
            rakazo,
            front,
            EMAIL,
            password,
            Fillable {
                model: Some(ModelFill { model, due }),
                ..Default::default()
            },
            filled,
            &mut checklist,
        );
        checklist.items().map(str::to_string).collect()
    }

    fn with_owner() -> FakeRakazo {
        FakeRakazo {
            owner: RefCell::new(true),
            ..Default::default()
        }
    }

    fn scripted() -> crate::answers::NoQuestions<fn(&str) -> Option<String>> {
        crate::answers::NoQuestions { env: |_| None }
    }

    #[test]
    fn a_first_run_adds_the_new_owners_connection_as_the_main_model() {
        let rakazo = FakeRakazo::default();
        let (mut filled, mut due) = (Vec::new(), Some(Due::NewKey));
        let items = fill_model(
            &rakazo,
            &mut Silent,
            Some(password()),
            Provider::Gemini,
            &mut due,
            &mut filled,
        );
        assert_eq!(
            *rakazo.calls.borrow(),
            [
                "detect",
                "bootstrap-owner",
                "sign-in",
                "connect model",
                "sign-out"
            ]
        );
        assert_eq!(
            *rakazo.connects.borrow(),
            [("gemini", "gemini-3.5-flash-lite".to_string(), Connect::Add)]
        );
        assert_eq!(
            filled,
            [DatabaseSetting::Owner, DatabaseSetting::ModelConnection]
        );
        assert_eq!(due, None);
        assert_eq!(items.len(), 1, "only the standing item: {items:?}");
    }

    #[test]
    fn a_connection_that_has_the_key_is_left_alone_without_a_password() {
        let rakazo = with_owner();
        rakazo.connections.borrow_mut().push(Provider::OpenAi);
        for mut filled in [vec![], vec![DatabaseSetting::ModelConnection]] {
            rakazo.calls.borrow_mut().clear();
            let before = filled.clone();
            fill_model(
                &rakazo,
                &mut Silent,
                None,
                Provider::OpenAi,
                &mut None,
                &mut filled,
            );
            assert_eq!(*rakazo.calls.borrow(), ["detect", "detect model"]);
            assert_eq!(filled, before);
        }
    }

    #[test]
    fn a_new_key_updates_the_connection_and_only_a_new_provider_makes_it_the_main_model() {
        for (reason, make_default) in [(Due::NewKey, false), (Due::NewProvider, true)] {
            let rakazo = with_owner();
            rakazo.connections.borrow_mut().push(Provider::OpenAi);
            let (mut filled, mut due) = (Vec::new(), Some(reason));
            fill_model(
                &rakazo,
                &mut Silent,
                Some(password()),
                Provider::OpenAi,
                &mut due,
                &mut filled,
            );
            let update = Connect::Update {
                id: "openai-connection".into(),
                make_default,
            };
            assert_eq!(
                *rakazo.connects.borrow(),
                [("openai", "gpt-6-luna".to_string(), update)]
            );
            assert_eq!(due, None);
            assert_eq!(filled, [DatabaseSetting::ModelConnection]);
        }
    }

    #[test]
    fn a_failed_connect_stays_due_and_gives_the_ui_steps() {
        let rakazo = FakeRakazo {
            model_error: Some("Your OpenAI rejected the API key. (HTTP 400)".into()),
            ..with_owner()
        };
        let (mut filled, mut due) = (Vec::new(), Some(Due::NewProvider));
        let items = fill_model(
            &rakazo,
            &mut Silent,
            Some(password()),
            Provider::OpenAi,
            &mut due,
            &mut filled,
        );
        assert!(filled.is_empty());
        assert_eq!(due, Some(Due::NewProvider));
        assert_eq!(rakazo.calls.borrow().last().unwrap(), &"sign-out");
        assert_eq!(
            items[0],
            "OpenAI chat wasn't connected for the Owner: Your OpenAI rejected the API key. (HTTP 400). Run `rakazo-stack setup` again, or in Rakazo open Settings → Models, connect OpenAI with your API key and choose the model gpt-6-luna."
        );

        rakazo.connections.borrow_mut().push(Provider::OpenAi);
        let mut due = Some(Due::NewKey);
        let items = fill_model(
            &rakazo,
            &mut scripted(),
            None,
            Provider::OpenAi,
            &mut due,
            &mut filled,
        );
        assert_eq!(due, Some(Due::NewKey));
        assert!(
            items[0].starts_with("The Owner's OpenAI connection didn't get the new key: no Owner password was given (set RAKAZO_STACK_OWNER_PASSWORD).")
                && items[0].contains("enter the key on the OpenAI connection"),
            "{items:?}"
        );
    }

    #[test]
    fn a_removed_connection_comes_back_only_when_the_wizard_says_yes() {
        let rakazo = with_owner();
        let filled = vec![DatabaseSetting::ModelConnection];
        fill_model(
            &rakazo,
            &mut scripted(),
            Some(password()),
            Provider::Gemini,
            &mut Some(Due::NewKey),
            &mut filled.clone(),
        );
        assert_eq!(*rakazo.calls.borrow(), ["detect", "detect model"]);

        let mut yes = Wizard {
            prompt: Answers(vec!["y", "correct-horse-battery", "correct-horse-battery"]),
        };
        fill_model(
            &rakazo,
            &mut yes,
            None,
            Provider::Gemini,
            &mut None,
            &mut filled.clone(),
        );
        assert_eq!(rakazo.connects.borrow()[0].2, Connect::Add);
    }

    fn settings_on(provider: Option<Provider>) -> Settings {
        let mut settings = Settings::new(&crate::resolve::RakazoRelease {
            tag: "v0.16.0".into(),
            app_digest: "sha256:aaa".into(),
            computer_digest: "sha256:ccc".into(),
        });
        settings.model_provider = provider.map(|p| Choice(Some(p)));
        settings
    }

    /// What setup does on each run: note the change, then record the new choice.
    fn choose(settings: &mut Settings, provider: Option<Provider>, new_key: bool) {
        note_model_change(settings, Choice(provider), new_key);
        settings.model_provider = Some(Choice(provider));
    }

    #[test]
    fn a_new_key_or_provider_is_due_until_it_is_connected() {
        let mut settings = settings_on(None);
        choose(&mut settings, Some(Provider::OpenAi), true);
        assert_eq!(settings.model_connection_due, Some(Due::NewKey));
        settings.model_connection_due = None;
        choose(&mut settings, Some(Provider::OpenAi), false);
        assert_eq!(settings.model_connection_due, None);

        settings.filled_by_setup = vec![DatabaseSetting::Owner, DatabaseSetting::ModelConnection];
        choose(&mut settings, Some(Provider::Gemini), true);
        assert_eq!(settings.model_connection_due, Some(Due::NewProvider));
        assert_eq!(settings.filled_by_setup, [DatabaseSetting::Owner]);
        // A switch that hasn't connected yet still makes it the main model later.
        choose(&mut settings, Some(Provider::Gemini), true);
        assert_eq!(settings.model_connection_due, Some(Due::NewProvider));
        choose(&mut settings, Some(Provider::Gemini), false);
        assert_eq!(settings.model_connection_due, Some(Due::NewProvider));

        choose(&mut settings, Some(Provider::OpenRouter), true);
        assert_eq!(settings.model_connection_due, None);
    }

    #[test]
    fn providers_the_instance_left_are_remembered_until_chosen_again() {
        let openai = Choice(Some(Provider::OpenAi));
        let gemini = Choice(Some(Provider::Gemini));
        let mut settings = settings_on(Some(Provider::OpenRouter));
        choose(&mut settings, Some(Provider::OpenAi), true);
        assert!(settings.model_providers_left.is_empty());
        choose(&mut settings, Some(Provider::Gemini), true);
        assert_eq!(settings.model_providers_left, [openai]);
        choose(&mut settings, None, false);
        assert_eq!(settings.model_providers_left, [openai, gemini]);
        choose(&mut settings, Some(Provider::OpenAi), true);
        assert_eq!(settings.model_providers_left, [gemini]);
    }

    #[test]
    fn switching_away_lists_the_old_connection_until_it_is_removed() {
        let openai = Choice(Some(Provider::OpenAi));
        let check = |rakazo: &FakeRakazo, chosen, left: &mut Vec<Choice>| {
            let mut checklist = Checklist::default();
            old_connections(rakazo, EMAIL, chosen, left, &mut checklist);
            checklist.items().map(str::to_string).collect::<Vec<_>>()
        };
        let rakazo = with_owner();
        rakazo.connections.borrow_mut().push(Provider::OpenAi);
        let mut left = vec![openai];
        let items = check(&rakazo, Choice(Some(Provider::OpenRouter)), &mut left);
        assert_eq!(
            items[0],
            "You switched away from OpenAI: remove the old connection in Settings → Models, because until then it stays the Owner's default."
        );
        let items = check(&rakazo, Choice(Some(Provider::Gemini)), &mut left);
        assert_eq!(
            items[0],
            "You switched away from OpenAI: remove the old connection in Settings → Models."
        );
        assert_eq!(left, [openai]);

        rakazo.connections.borrow_mut().clear();
        let items = check(&rakazo, Choice(None), &mut left);
        assert_eq!(items.len(), 1, "only the standing item: {items:?}");
        assert!(left.is_empty());
    }
}
