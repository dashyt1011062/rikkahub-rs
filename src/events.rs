use tokio::sync::broadcast;

#[derive(Clone, Debug)]
pub enum AppEvent {
    ConversationChanged {
        account_id: String,
        conversation_id: String,
    },
    ConversationListInvalidated {
        account_id: String,
        assistant_id: String,
    },
    ConversationError {
        account_id: String,
        conversation_id: String,
        message: String,
    },
}

#[derive(Clone)]
pub struct EventHub {
    sender: broadcast::Sender<AppEvent>,
}

impl EventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.sender.subscribe()
    }

    pub fn emit(&self, event: AppEvent) {
        let _ = self.sender.send(event);
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

