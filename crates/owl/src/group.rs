//! The group operations flock fuzzes, behind one seam.
//!
//! flock targets GroupV3, which `create_group_conversation` creates. The
//! client exposes no epoch fingerprint yet. Its stand-in is a hash of the
//! member list as this owl sees it: members are only ever added, so two owls
//! that disagree about who is in still show up as a fork.

use std::collections::HashSet;

use flock_proto::GROUP_NAME;
use logos_chat::{GroupMetadata, LogosChatClient};

/// What roost shows as the group's type.
pub const CONVO_TYPE: &str = "GroupV3";

pub trait Groups {
    /// Creates a group with only this owl in it.
    fn create_group(&mut self) -> Result<String, String>;
    fn add_member(&mut self, group: &str, account: &str) -> Result<(), String>;
    /// Committed members' accounts.
    fn member_accounts(&self, group: &str) -> Result<HashSet<String>, String>;
    /// Accounts invited whose commit has not landed.
    fn pending_accounts(&self, group: &str) -> Result<HashSet<String>, String>;
    fn is_flock_group(&self, group: &str) -> bool;
    fn can_send(&self, group: &str) -> bool;
    fn send(&mut self, group: &str, content: &[u8]) -> Result<(), String>;
}

impl Groups for LogosChatClient {
    fn create_group(&mut self) -> Result<String, String> {
        let metadata = GroupMetadata {
            name: GROUP_NAME.into(),
            desc: String::new(),
        };
        self.create_group_conversation(&[], metadata)
            .map(|id| id.to_string())
            .map_err(|e| e.to_string())
    }

    fn add_member(&mut self, group: &str, account: &str) -> Result<(), String> {
        self.add_group_participants(group, &[account])
            .map_err(|e| e.to_string())
    }

    fn member_accounts(&self, group: &str) -> Result<HashSet<String>, String> {
        Ok(self
            .participants(group)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|a| a.to_string())
            .collect())
    }

    fn pending_accounts(&self, group: &str) -> Result<HashSet<String>, String> {
        Ok(self
            .pending_members(group)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|s| s.account.to_string())
            .collect())
    }

    fn is_flock_group(&self, group: &str) -> bool {
        self.group_metadata(group)
            .is_ok_and(|meta| meta.name == GROUP_NAME)
    }

    fn can_send(&self, group: &str) -> bool {
        LogosChatClient::can_send(self, group)
    }

    fn send(&mut self, group: &str, content: &[u8]) -> Result<(), String> {
        self.send_message(group, content)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Where an owl is in the group's history, from the members it sees. Take
/// both from one read of the member list: libchat's state moves underneath.
pub fn fingerprint(members: &HashSet<String>) -> String {
    let mut sorted: Vec<&String> = members.iter().collect();
    sorted.sort();
    let mut hash = crc32fast::Hasher::new();
    for m in sorted {
        hash.update(m.as_bytes());
        hash.update(&[0]);
    }
    format!("{:08x}", hash.finalize())
}
