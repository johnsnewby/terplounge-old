use crate::error::E;
use crossbeam_channel::{unbounded, Receiver, Sender};
use lazy_static::lazy_static;

use crate::translate::{TranslationRequest, Translator};

#[derive(Clone)]
pub struct TranslationQueue {
    sender: Sender<TranslationRequest>,
    receiver: Receiver<TranslationRequest>,
}

lazy_static! {
    pub static ref QUEUE: TranslationQueue = TranslationQueue::new().unwrap();
}

impl TranslationQueue {
    pub fn new() -> E<Self> {
        let (sender, receiver) = unbounded();
        Ok(Self { sender, receiver })
    }

    pub fn enqueue(&self, request: TranslationRequest) -> E<()> {
        log::debug!(
            "Enqueuing request for session with id {}",
            request.session_id
        );
        self.sender.send(request)?;
        log::debug!("Done");
        Ok(())
    }

    pub async fn queue_process(&self, rx: Receiver<TranslationRequest>) -> E<()> {
        for translation_request in rx.iter() {
            self.sender.send(translation_request)?;
        }
        Ok(())
    }

    pub fn subscribe<T: Translator>(&self, translator: &T) -> E<()> {
        loop {
            let req = self.receiver.recv()?;
            log::debug!("Queue length: {}", self.receiver.len());
            translator.translate(req)?;
        }
    }
}

pub fn get_queue() -> TranslationQueue {
    (*QUEUE).clone()
}
