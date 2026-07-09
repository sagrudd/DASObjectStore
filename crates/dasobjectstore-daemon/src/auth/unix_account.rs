use super::DaemonLocalActor;
use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io;

#[cfg(target_os = "linux")]
const PASSWD_PATH: &str = "/etc/passwd";
#[cfg(target_os = "linux")]
const GROUP_PATH: &str = "/etc/group";

#[derive(Clone, Debug, Eq, PartialEq)]
struct PasswdEntry {
    username: String,
    uid: u32,
    gid: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GroupEntry {
    group_name: String,
    gid: u32,
    members: Vec<String>,
}

#[cfg(target_os = "linux")]
pub fn actor_from_system_accounts(uid: u32, primary_gid: u32) -> io::Result<DaemonLocalActor> {
    let passwd = fs::read_to_string(PASSWD_PATH)?;
    let group = fs::read_to_string(GROUP_PATH)?;
    Ok(actor_from_account_text(uid, primary_gid, &passwd, &group))
}

pub fn actor_from_account_text(
    uid: u32,
    fallback_primary_gid: u32,
    passwd: &str,
    group: &str,
) -> DaemonLocalActor {
    let passwd_entry = parse_passwd(passwd)
        .into_iter()
        .find(|entry| entry.uid == uid);
    let username = passwd_entry.as_ref().map(|entry| entry.username.clone());
    let primary_gid = passwd_entry
        .as_ref()
        .map(|entry| entry.gid)
        .unwrap_or(fallback_primary_gid);
    let groups = groups_for_actor(username.as_deref(), primary_gid, &parse_group(group));

    let actor = DaemonLocalActor::new(uid).with_primary_gid(primary_gid);
    let actor = match username {
        Some(username) => actor.with_username(username),
        None => actor,
    };
    actor.with_groups(groups)
}

fn groups_for_actor(
    username: Option<&str>,
    primary_gid: u32,
    group_entries: &[GroupEntry],
) -> Vec<String> {
    let mut groups = BTreeSet::new();

    for entry in group_entries {
        if entry.gid == primary_gid {
            groups.insert(entry.group_name.clone());
        }
        if let Some(username) = username {
            if entry.members.iter().any(|member| member == username) {
                groups.insert(entry.group_name.clone());
            }
        }
    }

    groups.into_iter().collect()
}

fn parse_passwd(contents: &str) -> Vec<PasswdEntry> {
    contents
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let username = fields.next()?.to_string();
            let _password = fields.next()?;
            let uid = fields.next()?.parse().ok()?;
            let gid = fields.next()?.parse().ok()?;
            Some(PasswdEntry { username, uid, gid })
        })
        .collect()
}

fn parse_group(contents: &str) -> Vec<GroupEntry> {
    contents
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let group_name = fields.next()?.to_string();
            let _password = fields.next()?;
            let gid = fields.next()?.parse().ok()?;
            let members = fields
                .next()
                .unwrap_or_default()
                .split(',')
                .filter(|member| !member.trim().is_empty())
                .map(ToString::to_string)
                .collect();
            Some(GroupEntry {
                group_name,
                gid,
                members,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::actor_from_account_text;

    #[test]
    fn resolves_username_primary_group_and_supplemental_groups() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\nstephen:x:1000:1000:Stephen:/home/stephen:/bin/bash\n";
        let group = "root:x:0:\nstephen:x:1000:\nmnemosyne:x:1001:stephen,operator\ndasobjectstore:x:1002:stephen\n";

        let actor = actor_from_account_text(1000, 9999, passwd, group);

        assert_eq!(actor.username.as_deref(), Some("stephen"));
        assert_eq!(actor.primary_gid, Some(1000));
        assert_eq!(
            actor.groups,
            vec![
                "dasobjectstore".to_string(),
                "mnemosyne".to_string(),
                "stephen".to_string(),
            ]
        );
    }

    #[test]
    fn falls_back_to_peer_gid_when_uid_is_unknown() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n";
        let group = "users:x:100:\n";

        let actor = actor_from_account_text(2000, 100, passwd, group);

        assert_eq!(actor.username, None);
        assert_eq!(actor.primary_gid, Some(100));
        assert_eq!(actor.groups, vec!["users".to_string()]);
    }
}
