use chrono::{DateTime, Utc};
use crossbeam_channel::{unbounded, Sender};
use futures_util::{SinkExt, StreamExt, TryFutureExt};
use lazy_static::lazy_static;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::RwLock;
use uuid::Uuid;
use warp::ws::{Message, WebSocket};

use crate::error::E;
use crate::queue::{self};
use crate::translate::{self, TranslationResponse, TranslationResponses};

pub type Sessions = HashMap<usize, SessionData>;

/// Our global unique user id counter.
static NEXT_USER_ID: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Debug, Serialize)]
pub struct SessionData {
    #[serde(skip_serializing)]
    pub sender: Option<Sender<Message>>,
    #[serde(skip_serializing)]
    pub translator: Sender<translate::TranslationRequest>,
    pub language: String,
    pub uuid: Uuid,
    pub resource: Option<String>,
    pub sample_rate: u32,
    pub valid: bool,
    #[serde(skip_serializing)]
    pub buffer: Vec<f32>,
    pub sequence_number: u32,
    pub last_sequence: Option<u32>,
    pub recording: bool,
    #[serde(skip_serializing)]
    pub recording_file: Option<String>,
    #[serde(skip_serializing)]
    pub translations: Arc<Mutex<TranslationResponses>>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl SessionData {
    fn new(
        sender: Sender<Message>,
        translator: Sender<translate::TranslationRequest>,
        language: String,
        sample_rate: u32,
    ) -> Self {
        let uuid = Uuid::new_v4();
        let recording_file = match std::env::var("RECORDINGS_DIR") {
            Ok(dir) => Some(format!("{}/{}.wav", dir, uuid)),
            Err(_) => None,
        };

        Self {
            sender: Some(sender),
            translator,
            language,
            sample_rate,
            uuid,
            resource: None,
            recording: recording_file.is_some(),
            recording_file,
            valid: true,
            buffer: Vec::new(),
            sequence_number: 0,
            last_sequence: None,
            translations: Arc::new(Mutex::new(TranslationResponses::new())),
            updated_at: Utc::now(),
            created_at: Utc::now(),
        }
    }

    pub fn send_uuid(&mut self) -> E<()> {
        self.sender
            .as_ref()
            .ok_or("couldn't find sender")?
            .send(Message::text(
                json!({ "uuid": self.uuid.to_string() }).to_string(),
            ))?;
        Ok(())
    }

    pub fn send_translation(&mut self, response: &TranslationResponse) -> E<()> {
        log::debug!("Sending {:?} to queue", response);
        self.sender
            .as_ref()
            .ok_or("couldn't find sender")?
            .send(Message::text(json!(response).to_string()))?;
        self.translations
            .lock()
            .unwrap()
            .deref_mut()
            .add_translation(&response.clone())?;
        if let Some(last) = self.last_sequence
            && self.sequence_number >= last
        {
            log::debug!("Last sequence set and reached. Exiting");
            self.sender = None;
        }
        Ok(())
    }

    pub fn transcript(&self) -> E<String> {
        let mutex = self.translations.lock().unwrap();
        let responses: &crate::translate::TranslationResponses = mutex.deref();
        Ok(responses.to_string())
    }
}

lazy_static! {
    static ref WEBSOCKET_SEND_RUNTIME: Runtime = Builder::new_multi_thread()
        .worker_threads(10)
        .thread_name("user-runtime")
        .thread_stack_size(3 * 1024 * 1024)
        .build()
        .unwrap();
    static ref SYNC_BRIDGE_RUNTIME: Runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("sync-bridge-runtime")
        .thread_stack_size(3 * 1024 * 1024)
        .build()
        .unwrap();
    pub static ref SESSIONS: RwLock<Sessions> = RwLock::new(Sessions::default());
}

pub async fn get_session(id: &usize) -> Option<SessionData> {
    SESSIONS.write().await.get(id).cloned()
}

pub async fn get_sessions() -> Option<Vec<SessionData>> {
    Some(SESSIONS.read().await.iter().map(|x| x.1.clone()).collect())
}

pub fn get_session_sync(id: &usize) -> Option<SessionData> {
    let mut session: Option<SessionData> = None;
    SYNC_BRIDGE_RUNTIME.block_on(async {
        session = SESSIONS.write().await.get(id).cloned();
    });
    session
}

async fn set_session(id: usize, session: SessionData) {
    SESSIONS.write().await.insert(id, session);
}

// returns the id of the session with given uuid.
pub async fn find_session_with_uuid(uuid: &String) -> Option<usize> {
    for element in SESSIONS.read().await.iter() {
        if element.1.uuid.to_string().eq(uuid) {
            return Some(*element.0);
        }
    }
    None
}

pub async fn mutate_session<F>(id: &usize, mut f: F)
where
    F: FnMut(&mut SessionData),
{
    if let Some(x) = SESSIONS.write().await.get_mut(id) {
        f(x);
        x.updated_at = Utc::now();
    }
}

pub fn mutate_session_sync<F>(id: &usize, f: F)
where
    F: FnMut(&mut SessionData),
{
    SYNC_BRIDGE_RUNTIME.block_on(async {
        mutate_session(id, f).await;
    });
}

async fn remove_session(id: &usize) {
    let mut sessions = SESSIONS.write().await;
    sessions.remove(id);
}

pub async fn user_message(my_id: usize, msg: Message) -> E<()> {
    if !msg.is_binary() {
        // TODO: handle this
        return Ok(());
    }
    let data = msg.into_bytes();
    if let Some(session) = get_session(&my_id).await
        && let Some(_sender) = session.sender
    {
        let mut v: Vec<f32> = data
            .chunks_exact(4)
            .map(|a| f32::from_le_bytes([a[0], a[1], a[2], a[3]]))
            .collect();

        mutate_session(&my_id, |session| session.buffer.append(&mut v)).await;

        if let Some(pivot) = translate::find_silence(&session.buffer, session.sample_rate) {
            log::debug!("Sending to translate, pivot={}", pivot);
            let sequence_number = session.sequence_number;
            let payload = session.buffer[..pivot].to_vec();
            let lang = session.language;
            append_samples(
                session.recording_file,
                session.sample_rate,
                &session.buffer[..pivot],
            )?;
            let result = queue::get_queue().enqueue(translate::TranslationRequest {
                session_id: my_id,
                sequence_number,
                payload,
                lang,
            });
            match result {
                Ok(_) => {
                    drop(result);
                    mutate_session(&my_id, |session| {
                        session.buffer = session.buffer[pivot..].to_vec();
                        session.sequence_number += 1;
                    })
                    .await;
                }
                Err(_) => {
                    drop(result);
                    mutate_session(&my_id, |session| session.sender = None).await;
                }
            }
        }
    }
    Ok(())
}

pub async fn user_connected(
    ws: WebSocket,
    translate_tx: Sender<translate::TranslationRequest>,
    lang: String,
    sample_rate: u32,
) {
    let my_id = NEXT_USER_ID.fetch_add(1, Ordering::Relaxed);

    log::debug!("new chat user: {}", my_id);

    let (mut user_ws_tx, mut user_ws_rx) = ws.split();

    let (tx, rx) = unbounded();
    (*WEBSOCKET_SEND_RUNTIME).spawn(async move {
        let mut ok = true;
        for message in rx.iter() {
            log::debug!("Sending message");
            user_ws_tx
                .send(message)
                .unwrap_or_else(|e| {
                    log::debug!("websocket send error: {}", e);
                    ok = false;
                })
                .await;
            log::debug!("Message sent");
        }
        log::debug!("Exiting loop");
        user_ws_tx.close().await.unwrap();
    });

    let mut session = SessionData::new(tx, translate_tx, lang, sample_rate);
    session.send_uuid().unwrap();
    set_session(my_id, session).await;

    while let Some(result) = user_ws_rx.next().await {
        let msg = match result {
            Ok(msg) => msg,
            Err(e) => {
                log::debug!("websocket error(uid={}): {}", my_id, e);
                mutate_session(&my_id, |session| {
		    session.sender = None;
		    session.valid = false}).await;
                break;
            }
        };
        let session = get_session(&my_id).await;
        match session {
            Some(s) => {
                if !s.valid {
                    break;
                }
            }
            None => {
                log::warn!("Error getting session {}, bailing", my_id);
                break;
            }
        }
        let _ = user_message(my_id, msg).await;
    }

    user_disconnected(my_id).await;
}

pub async fn user_closing(uuid: String) {
    if let Some(session_id) = find_session_with_uuid(&uuid).await {
        let session = get_session(&session_id).await.unwrap();
        if session.sequence_number == 0 {
            mutate_session(&session_id, |session| {
                session.sender = None;
            })
            .await;
            return;
        }
        let last_sequence = session.sequence_number - 1;
        log::debug!(
            "Found session {}, marking it for closure at sequence number {}",
            session_id,
            last_sequence,
        );
        mutate_session(&session_id, |session| {
            session.last_sequence = Some(last_sequence)
        })
        .await;
    }
}

pub async fn user_disconnected(my_id: usize) {
    mutate_session(&my_id, |session| {
	session.sender = None;
	session.valid = false}).await;
    log::debug!("good bye user: {}", my_id);
}

pub async fn expire_sessions() -> E<()> {
    let now = Utc::now().timestamp();
    for (session_id, session) in (*SESSIONS).read().await.iter() {
        if now - session.updated_at.timestamp() > 86400 {
            remove_session(session_id).await;
        }
    }
    Ok(())
}

fn append_samples(filename: Option<String>, sample_rate: u32, buffer: &[f32]) -> E<()> {
    if let Some(filename) = filename {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let mut writer = if std::path::Path::exists(std::path::Path::new(&filename)) {
            hound::WavWriter::append(filename)?
        } else {
            hound::WavWriter::create(filename, spec)?
        };
        for sample in buffer {
            writer.write_sample(*sample).unwrap();
        }
    }
    Ok(())
}
