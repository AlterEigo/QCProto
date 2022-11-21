use serde::{Deserialize, Serialize};
use std::fmt::Display;

pub const PROTOCOL_VERSION: u16 = 102;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ActorInfos {
    pub server: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CommandKind {
    ForwardMessage {
        from: ActorInfos,
        to: ActorInfos,
        content: String,
    },
    GetOnlineUsers,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum BotFamily {
    Discord,
    Telegram,
    WhatsApp,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Command {
    pub kind: CommandKind,
    pub sender_bot_family: BotFamily,
    pub protocol_version: u16,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TransmissionResult {
    Received,
    BadSyntax,
    MismatchedVersions,
}

impl Display for TransmissionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            TransmissionResult::Received => write!(f, "Received")?,
            TransmissionResult::BadSyntax => write!(f, "Bad syntax")?,
            TransmissionResult::MismatchedVersions => write!(f, "Mismatched protocol versions")?,
        }
        Ok(())
    }
}

pub struct CommandContext;
