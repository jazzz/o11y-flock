//! The group operations an owl performs, behind one seam.
//!
//! flock targets GroupV3, which `create_group_conversation` creates. An
//! owl's fingerprint of a group is a hash of the member list as it sees it,
//! so two owls that disagree about who is in show up as a fork. Members are
//! added and removed, so a group can come back to a member list it had
//! before, and to its fingerprint: roost leaves out a commit back to an older
//! fingerprint. With it, the owl reports libchat's MLS epoch and that epoch's
//! fingerprint, which every member at the epoch shares unless the group
//! forked, and which never come back.
//!
//! libchat's member list leaves out a member whose auth fails or cannot be
//! decided, and logs why. Members change only in a new epoch, so at the same
//! epoch an owl takes a different list only if it adds members.

use std::collections::HashSet;

use flock_proto::GROUP_NAME;
use logos_chat::{GroupMetadata, LogosChatClient};

/// What roost shows as the group's type.
pub const CONVO_TYPE: &str = "GroupV3";

pub trait Groups {
    /// Creates a group with only this owl in it.
    fn create_group(&mut self) -> Result<String, String>;
    fn add_member(&mut self, group: &str, account: &str) -> Result<(), String>;
    /// Removes another member. libchat refuses to remove this owl itself.
    fn remove_member(&mut self, group: &str, account: &str) -> Result<(), String>;
    /// Committed members' accounts.
    fn member_accounts(&self, group: &str) -> Result<HashSet<String>, String>;
    /// Accounts invited whose commit has not landed.
    fn pending_accounts(&self, group: &str) -> Result<HashSet<String>, String>;
    /// The group's epoch, if libchat says.
    fn epoch(&self, group: &str) -> Option<Epoch>;
    fn is_flock_group(&self, group: &str) -> bool;
    fn can_send(&self, group: &str) -> bool;
    fn send(&mut self, group: &str, content: &[u8]) -> Result<(), String>;
}

/// An MLS epoch of a group, as libchat reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Epoch {
    pub number: u64,
    /// The same for every member at this epoch, unless the group forked.
    pub fingerprint: String,
}

/// One look at a group: its epoch, if libchat says, and its members.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub epoch: Option<Epoch>,
    pub members: HashSet<String>,
}

impl Reading {
    /// Whether to report this reading, after `last`, the one last reported.
    /// A list without this owl (`own`) is missing members that failed auth.
    /// At the same epoch, only a list that adds members is news: it
    /// corrects one reported short.
    pub fn is_news(&self, own: &str, last: Option<&Reading>) -> bool {
        if !self.members.contains(own) {
            return false;
        }
        let Some(last) = last else {
            return true;
        };
        match (&self.epoch, &last.epoch) {
            (Some(now), Some(then)) if now == then => {
                self.members.len() > last.members.len() && self.members.is_superset(&last.members)
            }
            (Some(_), _) => true,
            (None, _) => self.members != last.members,
        }
    }
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

    fn remove_member(&mut self, group: &str, account: &str) -> Result<(), String> {
        self.remove_group_participants(group, &[account])
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

    fn epoch(&self, group: &str) -> Option<Epoch> {
        let data = self.debug_data(group).ok()?;
        Some(Epoch {
            number: data.get("epoch")?.as_u64()?,
            fingerprint: data.get("epoch_fingerprint")?.as_str()?.to_string(),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(epoch: Option<u64>, members: &[&str]) -> Reading {
        Reading {
            epoch: epoch.map(|n| Epoch {
                number: n,
                fingerprint: format!("fp{n}"),
            }),
            members: members.iter().map(|m| m.to_string()).collect(),
        }
    }

    #[test]
    fn a_list_without_this_owl_is_never_news() {
        assert!(!reading(Some(1), &[]).is_news("a", None));
        assert!(!reading(Some(2), &["b"]).is_news("a", Some(&reading(Some(1), &["a", "b"]))));
    }

    #[test]
    fn the_first_reading_is_news() {
        assert!(reading(Some(1), &["a"]).is_news("a", None));
        assert!(reading(None, &["a"]).is_news("a", None));
    }

    #[test]
    fn a_new_epoch_is_news_even_with_the_same_members() {
        let last = reading(Some(1), &["a", "b"]);
        assert!(reading(Some(2), &["a", "b"]).is_news("a", Some(&last)));
        assert!(reading(Some(2), &["a"]).is_news("a", Some(&last)));
    }

    #[test]
    fn at_the_same_epoch_only_more_members_are_news() {
        let last = reading(Some(3), &["a", "b"]);
        assert!(!reading(Some(3), &["a", "b"]).is_news("a", Some(&last)));
        // b failed auth this time.
        assert!(!reading(Some(3), &["a"]).is_news("a", Some(&last)));
        // c failed auth last time.
        assert!(reading(Some(3), &["a", "b", "c"]).is_news("a", Some(&last)));
    }

    #[test]
    fn without_an_epoch_any_other_list_is_news() {
        let last = reading(None, &["a", "b"]);
        assert!(!reading(None, &["a", "b"]).is_news("a", Some(&last)));
        assert!(reading(None, &["a"]).is_news("a", Some(&last)));
    }
}
