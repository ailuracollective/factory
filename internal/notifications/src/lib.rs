//! Shared notification contract for Ailura Factory.
//!
//! Delivery is performed through Firebase (FCM). The *club* produces a
//! [`Notification`] and hands it to a Firebase sender; the *bot* receives the
//! resulting message and renders it in a chat channel.
//!
//! The contract is transport-agnostic: a `Notification` serializes to JSON and
//! can be embedded verbatim in an FCM data payload.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Originator of a notification. In the current design this is always a club.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sender {
    /// Stable identifier of the sending club.
    pub id: String,
    /// Human-readable club name.
    pub name: String,
    /// Optional avatar/logo URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

/// Destination of a notification. In the current design this is always a bot
/// that renders the message in a chat channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipient {
    /// Stable identifier of the receiving bot.
    pub bot_id: String,
    /// Chat channel where the bot must display the notification.
    pub channel: String,
}

/// High-level kind of notification. New variants are added as the product grows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// A factory agent finished work on an issue/PR.
    AgentCompleted,
    /// A factory agent failed.
    AgentFailed,
    /// A mention was received and is being handled.
    MentionReceived,
    /// Generic status update.
    Status,
    /// Free-form message.
    Custom(String),
}

/// The canonical notification contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// Unique notification id (UUID v4).
    pub id: String,
    /// Kind of notification.
    pub kind: NotificationKind,
    /// Short title shown in chat.
    pub title: String,
    /// Longer body shown in chat.
    pub body: String,
    /// Who sends it (the club).
    pub sender: Sender,
    /// Who receives and displays it (the bot).
    pub recipient: Recipient,
    /// Unix timestamp in milliseconds when the notification was created.
    pub created_at: i64,
    /// Extra structured data forwarded to the bot verbatim.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub data: BTreeMap<String, serde_json::Value>,
}

impl Notification {
    /// Build a new notification with a fresh id and current timestamp.
    pub fn new(
        kind: NotificationKind,
        title: impl Into<String>,
        body: impl Into<String>,
        sender: Sender,
        recipient: Recipient,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            title: title.into(),
            body: body.into(),
            sender,
            recipient,
            created_at: chrono::Utc::now().timestamp_millis(),
            data: BTreeMap::new(),
        }
    }

    /// Attach extra data.
    pub fn with_data(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.data.insert(key.into(), value);
        self
    }

    /// Validate the notification. Returns `Ok(())` when all required fields are present.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id.is_empty() {
            return Err(ValidationError::Empty("id"));
        }
        if self.title.is_empty() {
            return Err(ValidationError::Empty("title"));
        }
        if self.body.is_empty() {
            return Err(ValidationError::Empty("body"));
        }
        if self.sender.id.is_empty() || self.sender.name.is_empty() {
            return Err(ValidationError::Empty("sender"));
        }
        if self.recipient.bot_id.is_empty() || self.recipient.channel.is_empty() {
            return Err(ValidationError::Empty("recipient"));
        }
        if self.created_at <= 0 {
            return Err(ValidationError::Invalid("created_at"));
        }
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
    Empty(&'static str),
    Invalid(&'static str),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::Empty(field) => write!(f, "field `{}` must not be empty", field),
            ValidationError::Invalid(field) => write!(f, "field `{}` is invalid", field),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Firebase Cloud Messaging (FCM) representation of a [`Notification`].
///
/// The notification is carried in the FCM `data` payload as a JSON string under
/// the `notification` key, so the bot can deserialize it verbatim. Title and
/// body are also mirrored into the FCM `notification` block for OS-level display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FcmMessage {
    pub token: String,
    pub notification: FcmNotification,
    pub data: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FcmNotification {
    pub title: String,
    pub body: String,
}

impl Notification {
    /// Convert into an FCM message addressed to `device_token`.
    pub fn to_fcm(&self, device_token: impl Into<String>) -> FcmMessage {
        let json = serde_json::to_string(self).unwrap_or_default();
        let mut data = BTreeMap::new();
        data.insert("notification".to_string(), json);
        data.insert(
            "kind".to_string(),
            serde_json::to_string(&self.kind).unwrap_or_default(),
        );
        data.insert("channel".to_string(), self.recipient.channel.clone());
        FcmMessage {
            token: device_token.into(),
            notification: FcmNotification {
                title: self.title.clone(),
                body: self.body.clone(),
            },
            data,
        }
    }
}

/// Round-trip helper: parse a `Notification` from an FCM data map.
pub fn from_fcm_data(data: &BTreeMap<String, String>) -> Result<Notification, serde_json::Error> {
    let raw = data.get("notification").cloned().unwrap_or_default();
    serde_json::from_str(&raw)
}
